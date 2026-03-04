//! End-to-end encryption primitives for relay traffic.
//!
//! Uses X25519 ECDH for key exchange + AES-256-GCM for symmetric encryption.
//! The relay server only sees opaque encrypted blobs.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret};

/// E2E envelope types sent over the relay WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum E2EEnvelope {
    /// Browser → daemon: initiate key exchange.
    E2eHello {
        /// Browser's X25519 public key (base64).
        pk: String,
    },
    /// Daemon → browser: complete key exchange.
    E2eHelloAck {
        /// Daemon's X25519 public key (base64).
        pk: String,
        /// Safety number for MITM detection (6 hex chars).
        safety_number: String,
    },
    /// Encrypted application message (bidirectional).
    E2e {
        /// AES-GCM nonce (base64, 12 bytes).
        n: String,
        /// Ciphertext (base64).
        d: String,
    },
}

/// An established E2E session with AES-256-GCM encryption.
pub struct E2ESession {
    cipher: Aes256Gcm,
    nonce_counter: u64,
    pub safety_number: String,
}

impl E2ESession {
    /// Create a session from the ECDH shared secret and both public keys.
    ///
    /// The shared secret is expanded via HKDF-SHA256 to derive the AES key.
    pub fn from_shared_secret(
        shared: &SharedSecret,
        our_pk: &[u8; 32],
        their_pk: &[u8; 32],
    ) -> Self {
        let aes_key = derive_aes_key(shared.as_bytes());
        let cipher = Aes256Gcm::new_from_slice(&aes_key).expect("valid key length");
        let safety_number = compute_safety_number(our_pk, their_pk);
        Self {
            cipher,
            nonce_counter: 0,
            safety_number,
        }
    }

    /// Encrypt plaintext into an E2E envelope.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> E2EEnvelope {
        let nonce_bytes = self.next_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext)
            .expect("encryption should not fail");
        E2EEnvelope::E2e {
            n: B64.encode(nonce_bytes),
            d: B64.encode(ciphertext),
        }
    }

    /// Decrypt an E2E envelope back to plaintext.
    pub fn decrypt(&self, nonce_b64: &str, ciphertext_b64: &str) -> anyhow::Result<Vec<u8>> {
        let nonce_bytes = B64
            .decode(nonce_b64)
            .map_err(|e| anyhow::anyhow!("invalid nonce base64: {e}"))?;
        let ciphertext = B64
            .decode(ciphertext_b64)
            .map_err(|e| anyhow::anyhow!("invalid ciphertext base64: {e}"))?;

        if nonce_bytes.len() != 12 {
            anyhow::bail!("nonce must be 12 bytes, got {}", nonce_bytes.len());
        }

        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
    }

    /// Generate the next 12-byte nonce from the internal counter.
    fn next_nonce(&mut self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&self.nonce_counter.to_be_bytes());
        self.nonce_counter += 1;
        nonce
    }
}

/// Generate an X25519 ephemeral keypair.
/// Returns (secret, public_key_bytes).
pub fn generate_keypair() -> (EphemeralSecret, [u8; 32]) {
    let secret = EphemeralSecret::random_from_rng(aes_gcm::aead::OsRng);
    let public = PublicKey::from(&secret);
    (secret, public.to_bytes())
}

/// Perform ECDH key agreement.
pub fn ecdh(secret: EphemeralSecret, their_pk_bytes: &[u8; 32]) -> SharedSecret {
    let their_pk = PublicKey::from(*their_pk_bytes);
    secret.diffie_hellman(&their_pk)
}

/// Derive a 32-byte AES key from the ECDH shared secret via HKDF-SHA256.
fn derive_aes_key(shared_secret: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"lukan-e2e-v1"), shared_secret);
    let mut key = [0u8; 32];
    hk.expand(b"aes-256-gcm", &mut key)
        .expect("valid output length");
    key
}

/// Compute a safety number from two public keys for MITM detection.
///
/// SHA256(sorted(pk_a, pk_b))[0..3] → 6 hex chars.
/// Both sides compute the same value; if a MITM relay substitutes keys,
/// the numbers won't match.
pub fn compute_safety_number(pk_a: &[u8; 32], pk_b: &[u8; 32]) -> String {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    // Sort keys so both sides get the same result regardless of order
    if pk_a <= pk_b {
        hasher.update(pk_a);
        hasher.update(pk_b);
    } else {
        hasher.update(pk_b);
        hasher.update(pk_a);
    }
    let hash = hasher.finalize();
    format!("{:02x}{:02x}{:02x}", hash[0], hash[1], hash[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let (secret_a, pk_a) = generate_keypair();
        let (secret_b, pk_b) = generate_keypair();

        let shared_a = ecdh(secret_a, &pk_b);
        let shared_b = ecdh(secret_b, &pk_a);

        // Both sides derive the same shared secret
        assert_eq!(shared_a.as_bytes(), shared_b.as_bytes());

        let mut session_a = E2ESession::from_shared_secret(&shared_a, &pk_a, &pk_b);
        let session_b = E2ESession::from_shared_secret(&shared_b, &pk_b, &pk_a);

        // Safety numbers match
        assert_eq!(session_a.safety_number, session_b.safety_number);

        // Encrypt on A, decrypt on B
        let plaintext = b"hello from browser";
        let envelope = session_a.encrypt(plaintext);

        if let E2EEnvelope::E2e { n, d } = envelope {
            let decrypted = session_b.decrypt(&n, &d).unwrap();
            assert_eq!(decrypted, plaintext);
        } else {
            panic!("expected E2e envelope");
        }
    }

    #[test]
    fn test_safety_number_order_independent() {
        let pk_a = [1u8; 32];
        let pk_b = [2u8; 32];
        assert_eq!(
            compute_safety_number(&pk_a, &pk_b),
            compute_safety_number(&pk_b, &pk_a)
        );
    }

    #[test]
    fn test_safety_number_format() {
        let pk_a = [0u8; 32];
        let pk_b = [1u8; 32];
        let sn = compute_safety_number(&pk_a, &pk_b);
        assert_eq!(sn.len(), 6);
        assert!(sn.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
