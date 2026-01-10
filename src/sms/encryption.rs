#![cfg_attr(not(feature = "http-server"), allow(dead_code))]

use crate::sms::SMSEncryptionKey;
use aes_gcm::aead::Aead;
use aes_gcm::aes::Aes256;
use aes_gcm::{Aes256Gcm, AesGcm, KeyInit, Nonce};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose;
use base64::Engine;
use cipher::consts::U12;
use cipher::Key;
use rand::{rng, RngCore};

pub struct SMSEncryption {
    cipher: AesGcm<Aes256, U12>,
}
impl SMSEncryption {
    pub fn new(key: SMSEncryptionKey) -> Self {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        Self { cipher }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; 12];
        rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;

        let mut encrypted_data = nonce_bytes.to_vec();
        encrypted_data.extend_from_slice(&ciphertext);

        Ok(general_purpose::STANDARD.encode(&encrypted_data))
    }

    pub fn decrypt(&self, encrypted_data: &str) -> Result<String> {
        let encrypted_bytes = general_purpose::STANDARD
            .decode(encrypted_data)
            .map_err(|e| anyhow!("Base64 decode failed: {}", e))?;

        if encrypted_bytes.len() < 12 {
            return Err(anyhow!("Invalid encrypted data length"));
        }

        let (nonce_bytes, ciphertext) = encrypted_bytes.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;

        String::from_utf8(plaintext).map_err(|e| anyhow!("UTF-8 conversion failed: {}", e))
    }
}
