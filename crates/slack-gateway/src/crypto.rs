//! Encryption and digest primitives for gateway-owned credentials.

use aes_gcm_siv::aead::{Aead, KeyInit as AeadKeyInit};
// aes-gcm-siv 0.12 stopped re-exporting `OsRng` and `rand_core` through `aead`.
// The generator is taken from `rand` directly, which is what
// orchestrator-security's secret_store_crypto.rs has always done — so the two
// crypto call sites now name the same source of randomness instead of two.
use aes_gcm_siv::{Aes256GcmSiv, Nonce};
use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit as HmacKeyInit, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

const NONCE_LEN: usize = 12;

/// Gateway master-key facade used for envelope encryption and stable digests.
#[derive(Clone)]
pub struct GatewayCrypto {
    key: [u8; 32],
}

impl std::fmt::Debug for GatewayCrypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GatewayCrypto([REDACTED])")
    }
}

impl GatewayCrypto {
    /// Decodes a base64-encoded 32-byte master key.
    pub fn from_base64(value: &str) -> Result<Self> {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(value.trim())
            .context("gateway master key must be valid base64")?;
        if decoded.len() != 32 {
            bail!("gateway master key must decode to exactly 32 bytes");
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&decoded);
        Ok(Self { key })
    }

    /// Encrypts one secret with an explicit context binding.
    pub fn encrypt(&self, context: &str, plaintext: &str) -> Result<String> {
        let cipher = <Aes256GcmSiv as AeadKeyInit>::new_from_slice(&self.key)
            .map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
        let mut nonce = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let mut bound = context.as_bytes().to_vec();
        bound.push(0);
        bound.extend_from_slice(plaintext.as_bytes());
        let ciphertext = cipher
            .encrypt(&Nonce::from(nonce), bound.as_ref())
            .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;
        let mut envelope = nonce.to_vec();
        envelope.extend_from_slice(&ciphertext);
        Ok(URL_SAFE_NO_PAD.encode(envelope))
    }

    /// Decrypts one context-bound secret.
    pub fn decrypt(&self, context: &str, envelope: &str) -> Result<String> {
        let bytes = URL_SAFE_NO_PAD
            .decode(envelope)
            .context("credential envelope is invalid")?;
        if bytes.len() <= NONCE_LEN {
            bail!("credential envelope is truncated");
        }
        let cipher = <Aes256GcmSiv as AeadKeyInit>::new_from_slice(&self.key)
            .map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
        let plaintext = cipher
            .decrypt(
                // Length is checked above: a shorter envelope bails as truncated.
                // aes-gcm-siv 0.12 deprecates the panicking `from_slice`.
                &Nonce::try_from(&bytes[..NONCE_LEN])
                    .map_err(|_| anyhow::anyhow!("credential envelope nonce is malformed"))?,
                &bytes[NONCE_LEN..],
            )
            .map_err(|_| anyhow::anyhow!("credential decryption failed"))?;
        let prefix = [context.as_bytes(), &[0]].concat();
        if !plaintext.starts_with(&prefix) {
            bail!("credential context mismatch");
        }
        String::from_utf8(plaintext[prefix.len()..].to_vec())
            .context("credential plaintext is not UTF-8")
    }

    /// Produces a non-reversible stable digest scoped by a purpose label.
    pub fn digest(&self, purpose: &str, value: &str) -> String {
        let derived = Sha256::digest([self.key.as_slice(), purpose.as_bytes()].concat());
        let Ok(mut mac) = <HmacSha256 as HmacKeyInit>::new_from_slice(&derived) else {
            return String::new();
        };
        mac.update(value.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Compares a presented secret to a stored digest in constant time.
    pub fn verify_digest(&self, purpose: &str, value: &str, expected_hex: &str) -> bool {
        let Ok(expected) = hex::decode(expected_hex) else {
            return false;
        };
        let derived = Sha256::digest([self.key.as_slice(), purpose.as_bytes()].concat());
        let Ok(mut mac) = <HmacSha256 as HmacKeyInit>::new_from_slice(&derived) else {
            return false;
        };
        mac.update(value.as_bytes());
        mac.verify_slice(&expected).is_ok()
    }
}

/// Generates a URL-safe 256-bit one-time secret.
pub fn random_secret() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crypto() -> GatewayCrypto {
        GatewayCrypto { key: [7_u8; 32] }
    }

    #[test]
    fn encrypted_secrets_are_context_bound_and_randomized() {
        let first = crypto()
            .encrypt("installation:one", "xoxb-secret")
            .expect("encrypt");
        let second = crypto()
            .encrypt("installation:one", "xoxb-secret")
            .expect("encrypt");
        assert_ne!(first, second);
        assert_eq!(
            crypto()
                .decrypt("installation:one", &first)
                .expect("decrypt"),
            "xoxb-secret"
        );
        assert!(crypto().decrypt("installation:two", &first).is_err());
        assert!(!first.contains("xoxb-secret"));
    }

    #[test]
    fn stable_digests_are_purpose_scoped_and_verifiable() {
        let digest = crypto().digest("team", "T123");
        assert!(crypto().verify_digest("team", "T123", &digest));
        assert!(!crypto().verify_digest("team", "T999", &digest));
        assert_ne!(digest, crypto().digest("poll", "T123"));
    }
}
