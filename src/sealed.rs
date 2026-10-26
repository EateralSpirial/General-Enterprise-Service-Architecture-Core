//! Explicit headless vault: authenticated ciphertext and an owner-private local key.
use crate::config;
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use anyhow::{ensure, Context, Result};
use std::path::Path;
fn key(home: &Path) -> Result<Vec<u8>> {
    let path = home.join(".vault-key");
    if !path.exists() {
        config::write_private_file(&path, &rand::random::<[u8; 32]>())?;
    }
    let bytes = config::read_private_file(&path, 32)?;
    ensure!(bytes.len() == 32, "invalid local vault key");
    Ok(bytes)
}
pub fn seal(home: &Path, id: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let nonce = rand::random::<[u8; 12]>();
    let mut value = vec![1];
    value.extend_from_slice(&nonce);
    let cipher =
        Aes256Gcm::new_from_slice(&key(home)?).map_err(|_| anyhow::anyhow!("invalid vault key"))?;
    value.extend(
        cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: bytes,
                    aad: id.as_bytes(),
                },
            )
            .map_err(|_| anyhow::anyhow!("vault encryption failed"))?,
    );
    Ok(value)
}
pub fn open(home: &Path, id: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 29 && bytes[0] == 1,
        "invalid encrypted vault record"
    );
    let key = config::read_private_file(&home.join(".vault-key"), 32)
        .context("local vault key unavailable")?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("invalid vault key"))?;
    cipher
        .decrypt(
            Nonce::from_slice(&bytes[1..13]),
            Payload {
                msg: &bytes[13..],
                aad: id.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("vault authentication failed"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ciphertext_is_bound_to_profile_and_rejects_tampering() {
        let root =
            std::env::temp_dir().join(format!("gesac-vault-{}", crate::runtime::random_id()));
        config::ensure_private_dir(&root).unwrap();
        let mut encrypted = seal(&root, "one", b"synthetic-password").unwrap();
        assert!(!encrypted.windows(18).any(|w| w == b"synthetic-password"));
        assert_eq!(
            open(&root, "one", &encrypted).unwrap(),
            b"synthetic-password"
        );
        assert!(open(&root, "two", &encrypted).is_err());
        encrypted[15] ^= 1;
        assert!(open(&root, "one", &encrypted).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
