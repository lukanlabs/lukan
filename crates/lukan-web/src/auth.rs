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
}
