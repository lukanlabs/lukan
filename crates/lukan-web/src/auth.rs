use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const BLOCK_SIZE: usize = 64;

/// Compute HMAC-SHA256 manually using sha2 crate.
fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    // If key > block size, hash it first
    let key = if key.len() > BLOCK_SIZE {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };

    // Pad key to block size
    let mut padded_key = [0u8; BLOCK_SIZE];
    padded_key[..key.len()].copy_from_slice(&key);

    // Inner hash: SHA256((key XOR ipad) || data)
    let mut inner = Sha256::new();
    let ipad: Vec<u8> = padded_key.iter().map(|b| b ^ 0x36).collect();
    inner.update(&ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    // Outer hash: SHA256((key XOR opad) || inner_hash)
    let mut outer = Sha256::new();
    let opad: Vec<u8> = padded_key.iter().map(|b| b ^ 0x5c).collect();
    outer.update(&opad);
    outer.update(inner_hash);
    outer.finalize().to_vec()
}

/// Sign a payload with HMAC-SHA256, returning `base64url(json).base64url(sig)`.
pub fn create_auth_token(secret: &str, ttl_ms: u64) -> String {
    let iat = chrono::Utc::now().timestamp_millis() as u64;
    let payload = serde_json::json!({ "iat": iat, "ttl": ttl_ms });
    let data = serde_json::to_string(&payload).unwrap();
    let b64 = URL_SAFE_NO_PAD.encode(data.as_bytes());

    let sig_bytes = hmac_sha256(secret.as_bytes(), b64.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(&sig_bytes);

    format!("{b64}.{sig}")
}

/// Verify a token's HMAC signature and check TTL expiration.
pub fn verify_auth_token(token: &str, secret: &str) -> bool {
    let Some(dot) = token.find('.') else {
        return false;
    };
    let b64 = &token[..dot];
    let sig = &token[dot + 1..];

    // Verify signature
    let expected_bytes = hmac_sha256(secret.as_bytes(), b64.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(&expected_bytes);
    if sig != expected {
        return false;
    }

    // Decode and check TTL
    let Ok(json_bytes) = URL_SAFE_NO_PAD.decode(b64) else {
        return false;
    };
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&json_bytes) else {
        return false;
    };

    let Some(iat) = payload.get("iat").and_then(|v| v.as_u64()) else {
        return false;
    };
    let ttl_ms = payload
        .get("ttl")
        .and_then(|v| v.as_u64())
        .unwrap_or(24 * 60 * 60 * 1000);

    let now = chrono::Utc::now().timestamp_millis() as u64;
    now.saturating_sub(iat) <= ttl_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_verify() {
        let secret = "test-secret-key-123";
        let token = create_auth_token(secret, 60_000);
        assert!(verify_auth_token(&token, secret));
        assert!(!verify_auth_token(&token, "wrong-secret"));
    }

    #[test]
    fn test_invalid_token() {
        assert!(!verify_auth_token("invalid", "secret"));
        assert!(!verify_auth_token("abc.def", "secret"));
        assert!(!verify_auth_token("", "secret"));
    }

    #[test]
    fn test_token_has_dot_separator() {
        let token = create_auth_token("mysecret", 60_000);
        assert!(
            token.contains('.'),
            "Token should have a dot separator: {token}"
        );
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 2, "Token should have exactly 2 parts");
        assert!(!parts[0].is_empty(), "Payload part should not be empty");
        assert!(!parts[1].is_empty(), "Signature part should not be empty");
    }

    #[test]
    fn test_different_secrets_produce_different_tokens() {
        let token1 = create_auth_token("secret-a", 60_000);
        let token2 = create_auth_token("secret-b", 60_000);
        // Payloads might differ by iat, but signatures certainly differ
        let sig1 = token1.split('.').nth(1).unwrap();
        let sig2 = token2.split('.').nth(1).unwrap();
        assert_ne!(
            sig1, sig2,
            "Different secrets should produce different signatures"
        );
    }

    #[test]
    fn test_verify_rejects_tampered_payload() {
        let secret = "test-secret";
        let token = create_auth_token(secret, 60_000);
        // Tamper with the payload (change first char)
        let mut tampered = token.clone();
        let replacement = if &token[..1] == "A" { "B" } else { "A" };
        tampered.replace_range(..1, replacement);
        assert!(
            !verify_auth_token(&tampered, secret),
            "Tampered payload should not verify"
        );
    }

    #[test]
    fn test_verify_rejects_tampered_signature() {
        let secret = "test-secret";
        let token = create_auth_token(secret, 60_000);
        let dot = token.find('.').unwrap();
        // Tamper with the signature
        let payload = &token[..dot];
        let tampered = format!("{payload}.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert!(
            !verify_auth_token(&tampered, secret),
            "Tampered signature should not verify"
        );
    }

    #[test]
    fn test_token_payload_is_valid_base64_json() {
        let token = create_auth_token("secret", 60_000);
        let dot = token.find('.').unwrap();
        let b64_payload = &token[..dot];
        let decoded = URL_SAFE_NO_PAD.decode(b64_payload).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert!(json.get("iat").is_some(), "Payload should have iat field");
        assert!(json.get("ttl").is_some(), "Payload should have ttl field");
        assert_eq!(json["ttl"].as_u64().unwrap(), 60_000, "TTL should match");
    }

    #[test]
    fn test_hmac_sha256_with_long_key() {
        // Key longer than block size (64 bytes) should be hashed first
        let long_key = vec![0xABu8; 128];
        let data = b"test data";
        let result = hmac_sha256(&long_key, data);
        assert_eq!(result.len(), 32, "HMAC-SHA256 output should be 32 bytes");
        // Verify it's deterministic
        let result2 = hmac_sha256(&long_key, data);
        assert_eq!(result, result2);
    }

    #[test]
    fn test_hmac_sha256_with_short_key() {
        let short_key = b"key";
        let data = b"message";
        let result = hmac_sha256(short_key, data);
        assert_eq!(result.len(), 32);
        // Different data produces different HMAC
        let result2 = hmac_sha256(short_key, b"other message");
        assert_ne!(result, result2);
    }

    #[test]
    fn test_hmac_sha256_with_empty_data() {
        let key = b"secret";
        let result = hmac_sha256(key, b"");
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_verify_no_dot_returns_false() {
        assert!(!verify_auth_token("nodothere", "secret"));
    }

    #[test]
    fn test_verify_empty_parts() {
        // dot at start
        assert!(!verify_auth_token(".signature", "secret"));
        // dot at end
        assert!(!verify_auth_token("payload.", "secret"));
    }

    #[test]
    fn test_token_with_large_ttl() {
        let secret = "s";
        let big_ttl = u64::MAX / 2;
        let token = create_auth_token(secret, big_ttl);
        // Should still be valid since iat is ~now and TTL is enormous
        assert!(verify_auth_token(&token, secret));
    }
}
