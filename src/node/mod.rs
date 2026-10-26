//! Multi-host subscriptions, immutable packages and local lifecycle authority.
pub mod lifecycle;
pub mod uninstall;
use crate::{
    auth,
    config::{self, Config, Credentials},
    database,
    runtime::{self, ProductRelease, Signed},
};
use anyhow::{ensure, Context, Result};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub host_id: String,
    pub connection: Config,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub id: String,
    pub profile: String,
    pub plan: String,
    pub product: String,
    pub target: String,
    #[serde(default = "stable")]
    pub channel: String,
    #[serde(default)]
    pub adapter: Option<lifecycle::Adapter>,
    #[serde(default)]
    pub auto_update: bool,
}
fn stable() -> String {
    "stable".into()
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: String,
    pub instance: String,
    pub release_id: String,
    pub release: ProductRelease,
    pub directory: PathBuf,
    pub status: String,
    pub online: bool,
    #[serde(default)]
    pub reported: bool,
    #[serde(default)]
    pub result: Value,
}
pub struct Node {
    pub root: PathBuf,
    pub device: String,
    db: Connection,
    _lock: Option<File>,
}
impl Node {
    pub fn open(root: &Path) -> Result<Self> {
        config::ensure_private_dir(root)?;
        let marker = root.join("node.json");
        if !marker.exists() {
            config::write_private_file(
                &marker,
                &serde_json::to_vec(&json!({"schema":1,"device":runtime::random_id()}))?,
            )?;
        }
        let marker: Value = serde_json::from_slice(&config::read_private_file(&marker, 4096)?)?;
        let device = marker["device"]
            .as_str()
            .context("invalid node identity")?
            .to_owned();
        runtime::name(&device)?;
        let lock_path = root.join("node.lock");
        if !lock_path.exists() {
            config::write_private_file(&lock_path, &[])?;
        }
        config::read_private_file(&lock_path, 0)?;
        let lock = OpenOptions::new().read(true).write(true).open(lock_path)?;
        lock.try_lock_exclusive()
            .context("another node operation is active")?;
        let db = database::open(root, "node.sqlite")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS profiles(id TEXT PRIMARY KEY,body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS subscriptions(profile TEXT NOT NULL REFERENCES profiles(id),plan TEXT NOT NULL,PRIMARY KEY(profile,plan));
CREATE TABLE IF NOT EXISTS instances(id TEXT PRIMARY KEY,profile TEXT NOT NULL REFERENCES profiles(id),body TEXT NOT NULL,active TEXT);
CREATE TABLE IF NOT EXISTS releases(id TEXT PRIMARY KEY,body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS watermarks(stream TEXT PRIMARY KEY,sequence INTEGER NOT NULL,identity TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS operations(id TEXT PRIMARY KEY,instance TEXT NOT NULL REFERENCES instances(id),status TEXT NOT NULL,body TEXT NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS node_live_target ON operations(instance) WHERE status NOT IN ('applied','rolled_back','cancelled');
PRAGMA user_version=1;")?;
        Ok(Self {
            root: root.canonicalize()?,
            device,
            db,
            _lock: Some(lock),
        })
    }
    pub fn read(root: &Path) -> Result<Self> {
        config::ensure_private_dir(root)?;
        let marker: Value =
            serde_json::from_slice(&config::read_private_file(&root.join("node.json"), 4096)?)?;
        let device = marker["device"]
            .as_str()
            .context("invalid node identity")?
            .to_owned();
        runtime::name(&device)?;
        let db = Connection::open_with_flags(
            root.join("node.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self {
            root: root.canonicalize()?,
            device,
            db,
            _lock: None,
        })
    }
    pub fn dependency(&self, instance: &str, version: Option<&str>) -> Result<(Operation, bool)> {
        if let Ok(op) = self.active(instance) {
            if version.is_none_or(|v| v == op.release.version) {
                return Ok((op, false));
            }
        }
        let mut s=self.db.prepare("SELECT body FROM operations WHERE instance=?1 AND status='applying' ORDER BY rowid DESC")?;
        for row in s.query_map([instance], |r| r.get::<_, String>(0))? {
            let op: Operation = serde_json::from_str(&row?)?;
            if version.is_some_and(|v| v == op.release.version) {
                self.verify_operation_files(&op)?;
                return Ok((op, true));
            }
        }
        anyhow::bail!("managed version differs from running product")
    }
    pub fn add_profile(&self, profile: &Profile, credentials: &Credentials) -> Result<()> {
        runtime::name(&profile.id)?;
        runtime::name(&profile.host_id)?;
        profile.connection.validate()?;
        ensure!(
            self.db
                .query_row("SELECT 1 FROM profiles WHERE id=?1", [&profile.id], |r| r
                    .get::<_, u32>(
                    0
                ))
                .optional()?
                .is_none(),
            "profile already exists; remove it explicitly to change trust"
        );
        let home = self.profile_home(&profile.id)?;
        config::ensure_private_dir(&home)?;
        config::write_private_file(
            &home.join("config.json"),
            &serde_json::to_vec(&profile.connection)?,
        )?;
        config::store_credentials(&home, &profile.connection, credentials)?;
        self.db.execute(
            "INSERT INTO profiles VALUES (?1,?2)",
            params![profile.id, serde_json::to_string(profile)?],
        )?;
        Ok(())
    }
    pub fn profile(&self, id: &str) -> Result<Profile> {
        runtime::name(id)?;
        let body: String =
            self.db
                .query_row("SELECT body FROM profiles WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&body)?)
    }
    pub fn profile_home(&self, id: &str) -> Result<PathBuf> {
        runtime::name(id)?;
        Ok(self.root.join("profiles").join(id))
    }
    pub fn profiles(&self) -> Result<Value> {
        let mut s = self.db.prepare("SELECT body FROM profiles ORDER BY id")?;
        let rows = s.query_map([], |r| r.get::<_, String>(0))?;
        let mut values = Vec::new();
        for row in rows {
            let p: Profile = serde_json::from_str(&row?)?;
            values.push(json!({"id":p.id,"host_id":p.host_id,"url":p.connection.center_url,"account":p.connection.account}));
        }
        Ok(json!(values))
    }
    pub fn login(&self, id: &str, otp: Option<&str>) -> Result<()> {
        let p = self.profile(id)?;
        let home = self.profile_home(id)?;
        let c = config::load_credentials(&home, &p.connection)?;
        let mut value = json!({"account":p.connection.account,"password":c.password,"passphrase":c.passphrase,"device":self.device});
        if let Some(otp) = otp {
            value["otp"] = json!(otp);
        }
        let response = p
            .connection
            .client()?
            .post(p.connection.endpoint("/v1/login")?)
            .json(&value)
            .send()
            .map_err(|_| anyhow::anyhow!("host login unavailable"))?;
        ensure!(
            response.status().is_success(),
            "host login rejected; supply a current OTP if configured"
        );
        let mut bytes = Vec::new();
        response.take(16385).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 16384, "login response too large");
        let session: auth::Session = serde_json::from_slice(&bytes)?;
        ensure!(
            session.expires_at > runtime::now() + 5
                && session.expires_at <= runtime::now() + 86400
                && session.access_token.len() <= 256,
            "invalid session"
        );
        config::store_secret(
            &home,
            &p.connection,
            "session",
            &serde_json::to_vec(&session)?,
        )?;
        Ok(())
    }
    fn token(&self, p: &Profile) -> Result<String> {
        let home = self.profile_home(&p.id)?;
        if let Some(bytes) = config::read_secret(&home, &p.connection, "session")? {
            let session: auth::Session = serde_json::from_slice(&bytes)?;
            if session.expires_at > runtime::now() + 5 {
                return Ok(session.access_token.clone());
            }
        }
        self.login(&p.id, None)?;
        let session: auth::Session = serde_json::from_slice(
            &config::read_secret(&home, &p.connection, "session")?
                .context("session unavailable")?,
        )?;
        Ok(session.access_token.clone())
    }
    pub fn request(
        &self,
        profile: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        let p = self.profile(profile)?;
        let mut req = p
            .connection
            .client()?
            .request(method.parse()?, p.connection.endpoint(path)?)
            .bearer_auth(self.token(&p)?);
        if let Some(body) = body {
            req = req.json(body);
        }
        let response = req.send().map_err(|_| {
            anyhow::anyhow!("host request outcome unknown; reconcile original operation")
        })?;
        ensure!(
            response.status().is_success(),
            "host rejected request ({})",
            response.status().as_u16()
        );
        let mut bytes = Vec::new();
        response.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 4 * 1024 * 1024, "host response too large");
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub fn subscribe(&self, profile: &str, plan: &str) -> Result<()> {
        runtime::name(plan)?;
        let plans = self.request(profile, "GET", "/v1/plans", None)?;
        ensure!(
            plans["host_id"] == self.profile(profile)?.host_id
                && plans["plans"]
                    .as_array()
                    .context("invalid plans")?
                    .iter()
                    .any(|p| p["id"] == plan),
            "service plan unavailable"
        );
        self.db.execute(
            "INSERT OR IGNORE INTO subscriptions VALUES (?1,?2)",
            params![profile, plan],
        )?;
        Ok(())
    }
    pub fn register(&self, instance: &Instance) -> Result<()> {
        for n in [
            &instance.id,
            &instance.profile,
            &instance.plan,
            &instance.product,
            &instance.target,
            &instance.channel,
        ] {
            runtime::name(n)?;
        }
        ensure!(
            self.db
                .query_row(
                    "SELECT 1 FROM subscriptions WHERE profile=?1 AND plan=?2",
                    params![instance.profile, instance.plan],
                    |r| r.get::<_, u32>(0)
                )
                .optional()?
                .is_some(),
            "subscribe to the plan before registering an instance"
        );
        if let Some(adapter) = &instance.adapter {
            adapter.validate()?;
        }
        self.db.execute(
            "INSERT INTO instances(id,profile,body) VALUES (?1,?2,?3)",
            params![
                instance.id,
                instance.profile,
                serde_json::to_string(instance)?
            ],
        )?;
        Ok(())
    }
    pub fn instance(&self, id: &str) -> Result<Instance> {
        runtime::name(id)?;
        let b: String = self
            .db
            .query_row("SELECT body FROM instances WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&b)?)
    }
    pub fn offer(&self, id: &str) -> Result<Signed> {
        let i = self.instance(id)?;
        let mut url = url::Url::parse("https://local/v1/products/latest")?;
        url.query_pairs_mut()
            .append_pair("plan", &i.plan)
            .append_pair("product", &i.product)
            .append_pair("target", &i.target)
            .append_pair("channel", &i.channel);
        let signed: Signed = serde_json::from_value(self.request(
            &i.profile,
            "GET",
            &format!("{}?{}", url.path(), url.query().unwrap_or("")),
            None,
        )?)?;
        self.validate_release(&i, &signed, true)?;
        Ok(signed)
    }
    pub fn validate_release(
        &self,
        i: &Instance,
        signed: &Signed,
        new_admission: bool,
    ) -> Result<ProductRelease> {
        let p = self.profile(&i.profile)?;
        let release = signed.verify(&p.connection.release_public_key)?;
        ensure!(
            (
                &release.host_id,
                &release.plan,
                &release.product,
                &release.target,
                &release.channel
            ) == (&p.host_id, &i.plan, &i.product, &i.target, &i.channel),
            "release identity differs from instance"
        );
        if new_admission {
            ensure!(release.expires_at > runtime::now(), "release expired");
        }
        let stream = format!(
            "{}:{}:{}:{}:{}",
            i.profile, i.plan, i.product, i.target, i.channel
        );
        let prior: Option<(i64, String)> = self
            .db
            .query_row(
                "SELECT sequence,identity FROM watermarks WHERE stream=?1",
                [stream],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((seq, identity)) = prior {
            ensure!(
                release.sequence as i64 >= seq
                    && (release.sequence as i64 != seq || signed.identity()? == identity),
                "release replay or equivocation"
            );
        }
        Ok(release)
    }
    fn save(&self, op: &Operation) -> Result<()> {
        self.db.execute("INSERT INTO operations VALUES (?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET status=excluded.status,body=excluded.body",params![op.id,op.instance,op.status,serde_json::to_string(op)?])?;
        Ok(())
    }
    pub fn operation(&self, id: &str) -> Result<Operation> {
        let b: String =
            self.db
                .query_row("SELECT body FROM operations WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        Ok(serde_json::from_str(&b)?)
    }
    pub fn stage(
        &self,
        instance: &str,
        signed: Signed,
        offline: Option<&Path>,
    ) -> Result<Operation> {
        let i = self.instance(instance)?;
        let release = self.validate_release(&i, &signed, true)?;
        let release_id = signed.identity()?;
        if let Some(id)=self.db.query_row("SELECT id FROM operations WHERE instance=?1 AND json_extract(body,'$.release_id')=?2 ORDER BY rowid DESC LIMIT 1",params![instance,release_id],|r|r.get::<_,String>(0)).optional()? {return self.operation(&id);}
        let busy:u32=self.db.query_row("SELECT COUNT(*) FROM operations WHERE instance=?1 AND status NOT IN ('applied','rolled_back','cancelled')",[instance],|r|r.get(0))?;
        ensure!(busy == 0, "instance has unresolved work");
        let stream = format!(
            "{}:{}:{}:{}:{}",
            i.profile, i.plan, i.product, i.target, i.channel
        );
        self.db.execute("INSERT INTO watermarks VALUES (?1,?2,?3) ON CONFLICT(stream) DO UPDATE SET sequence=excluded.sequence,identity=excluded.identity",params![stream,release.sequence as i64,release_id])?;
        self.db.execute(
            "INSERT OR IGNORE INTO releases VALUES (?1,?2)",
            params![release_id, serde_json::to_string(&signed)?],
        )?;
        let directory = self.root.join("releases").join(&release_id);
        let mut op = Operation {
            id: format!("op-{}", runtime::random_id()),
            instance: instance.into(),
            release_id,
            release,
            directory,
            status: "staging".into(),
            online: offline.is_none(),
            reported: false,
            result: Value::Null,
        };
        self.save(&op)?;
        self.finish_stage(&i, &mut op, offline)?;
        Ok(op)
    }
    pub fn resume_stage(&self, id: &str, offline: Option<&Path>) -> Result<Operation> {
        let mut op = self.operation(id)?;
        ensure!(op.status == "staging", "operation is not staging");
        let i = self.instance(&op.instance)?;
        ensure!(op.online == offline.is_none(), "staging transport changed");
        self.finish_stage(&i, &mut op, offline)?;
        Ok(op)
    }
    fn finish_stage(&self, i: &Instance, op: &mut Operation, offline: Option<&Path>) -> Result<()> {
        if op.online {
            self.request(
                &i.profile,
                "POST",
                "/v1/operations",
                Some(&json!({"id":op.id,"instance":i.id,"release":op.release_id})),
            )?;
        }
        config::ensure_private_dir(&op.directory)?;
        let p = self.profile(&i.profile)?;
        for f in &op.release.files {
            let dest = op.directory.join(&f.path);
            if let Some(parent) = dest.parent() {
                config::ensure_private_dir(parent)?;
            }
            if dest.exists() && runtime::digest(&fs::read(&dest)?) == f.sha256 {
                continue;
            }
            let bytes = if let Some(bundle) = offline {
                config::read_private_file(&bundle.join("artifacts").join(&f.sha256), f.size)?
            } else {
                let response = p
                    .connection
                    .client()?
                    .get(p.connection.endpoint(&format!("/artifacts/{}", f.sha256))?)
                    .bearer_auth(self.token(&p)?)
                    .send()
                    .map_err(|_| anyhow::anyhow!("artifact download unavailable"))?;
                ensure!(response.status().is_success(), "artifact download rejected");
                let mut b = Vec::new();
                response.take(f.size + 1).read_to_end(&mut b)?;
                b
            };
            ensure!(
                bytes.len() as u64 == f.size && runtime::digest(&bytes) == f.sha256,
                "artifact integrity mismatch"
            );
            config::write_private_file(&dest, &bytes)?;
            if f.executable {
                config::harden_new_executable(&dest)?;
            }
        }
        let signed: String = self.db.query_row(
            "SELECT body FROM releases WHERE id=?1",
            [&op.release_id],
            |r| r.get(0),
        )?;
        config::write_private_file(&op.directory.join("gesac-release.json"), signed.as_bytes())?;
        op.status = "staged".into();
        self.save(op)?;
        Ok(())
    }
    pub fn verify_operation_files(&self, op: &Operation) -> Result<()> {
        let signed: Signed = serde_json::from_slice(&config::read_private_file(
            &op.directory.join("gesac-release.json"),
            4 * 1024 * 1024,
        )?)?;
        ensure!(
            signed.identity()? == op.release_id,
            "release receipt changed"
        );
        let i = self.instance(&op.instance)?;
        let p = self.profile(&i.profile)?;
        let verified = signed.verify(&p.connection.release_public_key)?;
        ensure!(
            serde_json::to_value(&verified)? == serde_json::to_value(&op.release)?,
            "release journal mismatch"
        );
        for f in &op.release.files {
            let path = op.directory.join(&f.path);
            let meta = fs::symlink_metadata(&path)?;
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() == f.size,
                "installed artifact changed"
            );
            ensure!(
                runtime::digest(&fs::read(path)?) == f.sha256,
                "installed artifact digest changed"
            );
        }
        Ok(())
    }
    pub fn export(&self, id: &str, to: &Path) -> Result<()> {
        let op = self.operation(id)?;
        self.verify_operation_files(&op)?;
        ensure!(!to.exists(), "export destination exists");
        config::ensure_private_dir(to)?;
        config::ensure_private_dir(&to.join("artifacts"))?;
        for f in &op.release.files {
            config::write_private_file(
                &to.join("artifacts").join(&f.sha256),
                &fs::read(op.directory.join(&f.path))?,
            )?;
        }
        config::write_private_file(
            &to.join("release.json"),
            &config::read_private_file(&op.directory.join("gesac-release.json"), 4 * 1024 * 1024)?,
        )?;
        Ok(())
    }
    pub fn active(&self, instance: &str) -> Result<Operation> {
        let id: Option<String> = self.db.query_row(
            "SELECT active FROM instances WHERE id=?1",
            [instance],
            |r| r.get(0),
        )?;
        let op = self.operation(&id.context("instance has no active release")?)?;
        ensure!(op.status == "applied", "active operation is unresolved");
        self.verify_operation_files(&op)?;
        Ok(op)
    }
    fn report(&self, op: &mut Operation) -> Result<()> {
        if op.online {
            let i = self.instance(&op.instance)?;
            self.request(
                &i.profile,
                "POST",
                &format!("/v1/operations/{}", op.id),
                Some(&json!({"status":op.status,"release":op.release_id,"result":op.result})),
            )?;
        }
        op.reported = true;
        self.save(op)
    }
    pub fn status(&self) -> Result<Value> {
        let mut st = self
            .db
            .prepare("SELECT i.id,i.body,i.active FROM instances i ORDER BY id")?;
        let mut instances = Vec::new();
        for row in st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })? {
            let (id, body, active) = row?;
            instances.push(
                json!({"id":id,"definition":serde_json::from_str::<Value>(&body)?,"active":active}),
            );
        }
        let mut st = self
            .db
            .prepare("SELECT body FROM operations ORDER BY rowid DESC LIMIT 100")?;
        let mut ops = Vec::new();
        for row in st.query_map([], |r| r.get::<_, String>(0))? {
            ops.push(serde_json::from_str::<Value>(&row?)?);
        }
        Ok(
            json!({"schema":1,"device":self.device,"profiles":self.profiles()?,"instances":instances,"operations":ops}),
        )
    }
    pub fn synchronize(&self) -> Result<Value> {
        let mut st = self.db.prepare("SELECT body FROM instances ORDER BY id")?;
        let rows: Vec<String> = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(st);
        let mut results = Vec::new();
        for body in rows {
            let i: Instance = serde_json::from_str(&body)?;
            if !i.auto_update {
                continue;
            }
            let result = (|| -> Result<Value> {
                let signed = self.offer(&i.id)?;
                if let Ok(active) = self.active(&i.id) {
                    if active.release_id == signed.identity()? {
                        return Ok(json!({"instance":i.id,"status":"current"}));
                    }
                }
                let mut op = self.stage(&i.id, signed, None)?;
                if op.status == "staged" {
                    op = self.transition(&op.id, "prepare")?;
                }
                if op.status == "prepared" {
                    op = self.transition(&op.id, "apply")?;
                }
                if ["unknown", "preparing", "applying", "rolling_back"]
                    .contains(&op.status.as_str())
                {
                    op = self.transition(&op.id, "reconcile")?;
                }
                Ok(json!({"instance":i.id,"status":op.status,"operation":op.id}))
            })();
            results.push(
                result.unwrap_or_else(|_| json!({"instance":i.id,"status":"needs_reconciliation"})),
            );
        }
        Ok(json!({"instances":results}))
    }
}
