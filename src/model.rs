use anyhow::Result;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, time::SystemTime};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> // GATE: atomic selection
{
    use std::io::Write;
    let mut file = atomic_write_file::AtomicWriteFile::open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.commit()?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub os: String,
    pub arch: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Release {
    pub schema: u32,
    pub product: String,
    pub version: String,
    pub target: String,
    pub sequence: i64,
    pub sha256: String,
    pub size: u64,
    pub format: String,
    pub entrypoint: Option<String>,
    pub arguments: Vec<String>,
    pub scope: String,
    pub created: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Signed {
    pub payload: String,
    pub signature: String,
}

impl Signed {
    pub fn create(value: &Release, key: &SigningKey) -> Result<Self> {
        let bytes = serde_json::to_vec(value)?;
        Ok(Self {
            payload: STANDARD.encode(&bytes),
            signature: hex::encode(key.sign(&bytes).to_bytes()),
        })
    }

    pub fn verify(&self, public_key: &str) -> Result<Release> // GATE: signed release authority
    {
        let bytes = STANDARD.decode(&self.payload)?;
        let bytes_key = hex::decode(public_key)?;
        let key = VerifyingKey::from_bytes(bytes_key.as_slice().try_into()?)?; // GATE
        key.verify(
            &bytes,
            &Signature::from_slice(&hex::decode(&self.signature)?)?,
        )?; // GATE
        Ok(serde_json::from_slice(&bytes)?)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub public_key: String,
    pub device: Device,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Login {
    pub account: String,
    pub password: String,
    pub device: Option<Device>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub token: String,
    pub account: String,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Installed {
    pub product: String,
    pub release: Release,
    pub operation: String,
    pub directory: String,
    pub auto_update: bool,
    pub previous: Option<Value>,
}
