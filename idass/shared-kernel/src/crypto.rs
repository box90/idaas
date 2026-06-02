use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};

use crate::error::AppError;

/// Encrypts `plaintext` with AES-GCM-256. Output is `nonce (12 bytes) || ciphertext`.
pub fn encrypt(master_key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, AppError> {
    let cipher = Aes256Gcm::new_from_slice(master_key).map_err(|_| AppError::Crypto)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher.encrypt(&nonce, plaintext).map_err(|_| AppError::Crypto)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypts bytes produced by `encrypt`. Input must be `nonce (12 bytes) || ciphertext`.
pub fn decrypt(master_key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, AppError> {
    if data.len() < 12 {
        return Err(AppError::Crypto);
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(master_key).map_err(|_| AppError::Crypto)?;
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext).map_err(|_| AppError::Crypto)
}

/// Loads the master key from `MASTER_ENCRYPTION_KEY` env var (base64-encoded 32 bytes).
/// Panics at startup if missing or wrong length.
pub fn master_key_from_env() -> [u8; 32] {
    let encoded = std::env::var("MASTER_ENCRYPTION_KEY")
        .expect("MASTER_ENCRYPTION_KEY env var required");
    let bytes = B64.decode(encoded).expect("MASTER_ENCRYPTION_KEY must be valid base64");
    bytes.try_into().expect("MASTER_ENCRYPTION_KEY must decode to exactly 32 bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let key = test_key();
        let plaintext = b"super-secret-client-secret";
        let ciphertext = encrypt(&key, plaintext).unwrap();
        let recovered = decrypt(&key, &ciphertext).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn different_nonce_each_call() {
        let key = test_key();
        let a = encrypt(&key, b"same plaintext").unwrap();
        let b = encrypt(&key, b"same plaintext").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn decrypt_wrong_key_returns_error() {
        let key = test_key();
        let wrong_key = [0x00u8; 32];
        let ct = encrypt(&key, b"secret").unwrap();
        assert!(decrypt(&wrong_key, &ct).is_err());
    }

    #[test]
    fn decrypt_truncated_data_returns_error() {
        let key = test_key();
        assert!(decrypt(&key, &[0u8; 5]).is_err());
    }

    #[test]
    fn decrypt_short_ciphertext_returns_error() {
        // 15 bytes: nonce (12) + ciphertext without GCM tag (< 16 bytes)
        let key = test_key();
        assert!(decrypt(&key, &[0u8; 15]).is_err());
    }
}
