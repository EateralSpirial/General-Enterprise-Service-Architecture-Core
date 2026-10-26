use super::*;
use hmac::{Hmac, Mac};
use scrypt::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Scrypt,
};
use sha1::Sha1;

pub fn hash(value: &str) -> Result<String> {
    Ok(Scrypt
        .hash_password(value.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|_| anyhow::anyhow!("credential hashing failed"))?
        .to_string())
}
fn matches(value: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .ok()
        .is_some_and(|h| Scrypt.verify_password(value.as_bytes(), &h).is_ok())
}
pub fn validate_seed(seed: &str) -> Result<()> {
    ensure!(
        (32..=128).contains(&seed.len())
            && seed.len().is_multiple_of(2)
            && hex::decode(seed).is_ok(),
        "OTP seed requires 16–64 hexadecimal bytes"
    );
    Ok(())
}
fn otp(seed: &str, counter: u64) -> Result<u32> {
    let mut mac = Hmac::<Sha1>::new_from_slice(&hex::decode(seed)?)?;
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[19] & 15) as usize;
    Ok((u32::from_be_bytes(hash[offset..offset + 4].try_into()?) & 0x7fffffff) % 1_000_000)
}
impl Service {
    pub(super) fn login(&mut self, body: &[u8]) -> Result<(StatusCode, Value)> {
        let value: Value = serde_json::from_slice(body)?;
        let account = field(&value, "account")?;
        let password = field(&value, "password")?;
        let passphrase = field(&value, "passphrase")?;
        ensure!(
            account.len() <= 96 && password.len() <= 4096 && passphrase.len() <= 4096,
            "unauthorized"
        );
        let device = value
            .get("device")
            .and_then(Value::as_str)
            .unwrap_or("legacy");
        runtime::name(device)?;
        let subject = runtime::digest(account.as_bytes());
        let now = runtime::now();
        let (count, until): (u32, i64) = self
            .db
            .query_row(
                "SELECT count,until_at FROM failures WHERE subject=?1",
                [&subject],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or((0, 0));
        ensure!(count < 5 || until <= now as i64, "unauthorized");
        self.db.execute("INSERT INTO failures VALUES (?1,1,?2) ON CONFLICT(subject) DO UPDATE SET count=CASE WHEN until_at<=?3 THEN 1 ELSE count+1 END,until_at=CASE WHEN until_at<=?3 THEN ?2 ELSE until_at END",params![subject,(now+60) as i64,now as i64])?;
        let row:Option<(String,String,Option<String>,i64)>=self.db.query_row("SELECT password,passphrase,otp_seed,last_otp FROM accounts WHERE name=?1 AND disabled=0",[account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (p, s, seed, last) = row.context("unauthorized")?;
        ensure!(
            matches(password, &p) & matches(passphrase, &s),
            "unauthorized"
        );
        if let Some(seed) = seed {
            let supplied = field(&value, "otp")?;
            ensure!(
                supplied.len() == 6 && supplied.bytes().all(|b| b.is_ascii_digit()),
                "unauthorized"
            );
            let n = supplied.parse::<u32>()?;
            let counter = now / 30;
            let accepted = [counter.saturating_sub(1), counter, counter + 1]
                .into_iter()
                .find(|c| *c as i64 > last && otp(&seed, *c).ok() == Some(n))
                .context("unauthorized")?;
            self.db.execute(
                "UPDATE accounts SET last_otp=?1 WHERE name=?2",
                params![accepted as i64, account],
            )?;
        }
        let token = runtime::random_id();
        let expiry = now + 900;
        self.db
            .execute("DELETE FROM sessions WHERE expires<=?1", [now as i64])?;
        self.db.execute(
            "INSERT INTO sessions VALUES (?1,?2,?3,?4)",
            params![
                runtime::digest(token.as_bytes()),
                account,
                device,
                expiry as i64
            ],
        )?;
        self.db
            .execute("DELETE FROM failures WHERE subject=?1", [subject])?;
        Ok((
            StatusCode::OK,
            json!({"access_token":token,"expires_at":expiry}),
        ))
    }
}
