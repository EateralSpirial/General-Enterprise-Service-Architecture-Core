//! Shared runtime contracts. Releases carry data; execution authority stays local.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_secs()
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn random_id() -> String {
    hex::encode(rand::random::<[u8; 24]>())
}
pub fn name(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 96
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && !value.contains(".."),
        "invalid identifier"
    );
    Ok(())
}
pub fn relative_file(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() < 512
            && !value.contains(['\\', ':', '\0'])
            && Path::new(value)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            && !value.starts_with('/'),
        "invalid release file path"
    );
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub executable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductRelease {
    pub schema: u32,
    pub host_id: String,
    pub plan: String,
    pub product: String,
    pub version: String,
    pub target: String,
    pub channel: String,
    pub sequence: u64,
    pub expires_at: u64,
    pub files: Vec<ReleaseFile>,
}
impl ProductRelease {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1 && self.sequence > 0 && self.sequence <= i64::MAX as u64,
            "invalid release schema/sequence"
        );
        for n in [
            &self.host_id,
            &self.plan,
            &self.product,
            &self.target,
            &self.channel,
        ] {
            name(n)?;
        }
        semver::Version::parse(&self.version)?;
        ensure!(
            !self.files.is_empty() && self.files.len() <= 10000,
            "invalid release file count"
        );
        let mut paths = std::collections::HashSet::new();
        let mut total = 0u64;
        for f in &self.files {
            relative_file(&f.path)?;
            ensure!(
                paths.insert(f.path.clone())
                    && f.sha256.len() == 64
                    && f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && f.size <= 256 * 1024 * 1024,
                "invalid release file"
            );
            total = total.checked_add(f.size).context("release too large")?;
        }
        ensure!(total <= 1024 * 1024 * 1024, "release exceeds 1 GiB");
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    pub payload: String,
    pub signature: String,
}
impl Signed {
    pub fn create<T: Serialize>(value: &T, key: &SigningKey) -> Result<Self> {
        let bytes = serde_json::to_vec(value)?;
        Ok(Self {
            payload: STANDARD.encode(&bytes),
            signature: hex::encode(key.sign(&bytes).to_bytes()),
        })
    }
    pub fn verify(&self, public_key: &str) -> Result<ProductRelease> {
        ensure!(
            self.payload.len() < 4 * 1024 * 1024,
            "release metadata too large"
        );
        let bytes = STANDARD.decode(&self.payload)?;
        let key: [u8; 32] = hex::decode(public_key)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid key"))?;
        VerifyingKey::from_bytes(&key)?.verify_strict(
            &bytes,
            &Signature::from_slice(&hex::decode(&self.signature)?)?,
        )?;
        let release: ProductRelease = serde_json::from_slice(&bytes)?;
        release.validate()?;
        Ok(release)
    }
    pub fn identity(&self) -> Result<String> {
        Ok(digest(&STANDARD.decode(&self.payload)?))
    }
}
