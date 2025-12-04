use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use dxid_core::Address;
use dxid_crypto::{address_from_string, address_to_string, generate_ed25519, DefaultCryptoProvider};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub name: String,
    pub address: Address,
    pub public_key: Vec<u8>,
    pub encrypted_secret: Vec<u8>,
    pub nonce: [u8; 12],
}

pub struct WalletStore {
    root: PathBuf,
    crypto: DefaultCryptoProvider,
}

impl WalletStore {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            crypto: DefaultCryptoProvider::new(),
        })
    }

    pub fn create(&self, name: &str, password: &str) -> Result<Wallet> {
        let kp = generate_ed25519();
        let address = self.crypto.address_from_public_key(&kp.public_key)?;
        let (encrypted_secret, nonce) = encrypt_secret(&kp.secret_key, password)?;
        let wallet = Wallet {
            name: name.to_string(),
            address,
            public_key: kp.public_key,
            encrypted_secret,
            nonce,
        };
        let path = self.root.join(format!("{name}.json"));
        fs::write(path, serde_json::to_vec_pretty(&wallet)?)?;
        Ok(wallet)
    }

    pub fn list(&self) -> Result<Vec<Wallet>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let bytes = fs::read(entry.path())?;
                let wallet: Wallet = serde_json::from_slice(&bytes)?;
                out.push(wallet);
            }
        }
        Ok(out)
    }

    pub fn load(&self, name: &str) -> Result<Wallet> {
        let path = self.root.join(format!("{name}.json"));
        let bytes = fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn unlock_secret(&self, wallet: &Wallet, password: &str) -> Result<Vec<u8>> {
        decrypt_secret(&wallet.encrypted_secret, &wallet.nonce, password)
    }
}

fn encrypt_secret(secret: &[u8], password: &str) -> Result<(Vec<u8>, [u8; 12])> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let mut key = [0u8; 32];
    pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, 10_000, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, secret)?;
    let mut out = salt.to_vec();
    out.extend_from_slice(&ciphertext);
    Ok((out, nonce_bytes))
}

fn decrypt_secret(ciphertext: &[u8], nonce: &[u8; 12], password: &str) -> Result<Vec<u8>> {
    if ciphertext.len() < 16 {
        return Err(anyhow!("ciphertext too short"));
    }
    let (salt, ct) = ciphertext.split_at(16);
    let mut key = [0u8; 32];
    pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, 10_000, &mut key);
    let cipher = Aes256Gcm::new_from_slice(&key)?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|e| anyhow!(format!("decrypt failed: {e}")))?;
    Ok(plaintext)
}

pub fn build_address_from_public_key(pk: &[u8]) -> Result<Address> {
    DefaultCryptoProvider::new().address_from_public_key(pk)
}

pub fn address_to_string_bech32(addr: &Address) -> String {
    address_to_string(addr)
}

pub fn address_from_bech32(s: &str) -> Result<Address> {
    address_from_string(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_roundtrip() {
        let store = WalletStore::new(std::env::temp_dir().join("dxid-wallet-test")).unwrap();
        let wallet = store.create("test", "pass").unwrap();
        let secret = store.unlock_secret(&wallet, "pass").unwrap();
        assert!(!secret.is_empty());
    }
}
