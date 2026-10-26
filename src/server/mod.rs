//! Private account/catalogue database and authenticated service-host API.
mod coordination;
mod security;
use crate::{
    config, database,
    runtime::{self, ProductRelease, ReleaseFile, Signed},
};
use anyhow::{ensure, Context, Result};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub host_id: String,
    pub listen: SocketAddr,
    pub redis_url: Option<String>,
    #[serde(default)]
    pub tls_cert: Option<PathBuf>,
    #[serde(default)]
    pub tls_key: Option<PathBuf>,
}
pub struct Service {
    pub root: PathBuf,
    pub config: ServerConfig,
    db: Connection,
}
type Shared = Arc<Mutex<Service>>;

impl Service {
    pub fn init(root: &Path, config: ServerConfig) -> Result<Self> {
        runtime::name(&config.host_id)?;
        ensure!(
            !root.join("server.json").exists(),
            "server already initialized"
        );
        config::ensure_private_dir(root)?;
        config::ensure_private_dir(&root.join("artifacts"))?;
        config::write_private_file(&root.join("signing-key"), &rand::random::<[u8; 32]>())?;
        config::write_private_file(
            &root.join("server.json"),
            &serde_json::to_vec_pretty(&config)?,
        )?;
        Self::open(root)
    }
    pub fn open(root: &Path) -> Result<Self> {
        let config: ServerConfig = serde_json::from_slice(&config::read_private_file(
            &root.join("server.json"),
            16384,
        )?)?;
        runtime::name(&config.host_id)?;
        let db = database::open(root, "server.sqlite")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS accounts (name TEXT PRIMARY KEY,password TEXT NOT NULL,passphrase TEXT NOT NULL,otp_seed TEXT,last_otp INTEGER NOT NULL DEFAULT -1,disabled INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS sessions (hash TEXT PRIMARY KEY,account TEXT NOT NULL REFERENCES accounts(name),device TEXT NOT NULL,expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS failures (subject TEXT PRIMARY KEY,count INTEGER NOT NULL,until_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS plans (id TEXT PRIMARY KEY,body TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS grants (account TEXT NOT NULL REFERENCES accounts(name),plan TEXT NOT NULL REFERENCES plans(id),PRIMARY KEY(account,plan));
CREATE TABLE IF NOT EXISTS releases (id TEXT PRIMARY KEY,plan TEXT NOT NULL REFERENCES plans(id),product TEXT NOT NULL,target TEXT NOT NULL,channel TEXT NOT NULL,sequence INTEGER NOT NULL,body TEXT NOT NULL,UNIQUE(plan,product,target,channel,sequence));
CREATE TABLE IF NOT EXISTS self_releases (target TEXT NOT NULL,channel TEXT NOT NULL,sequence INTEGER NOT NULL,sha256 TEXT NOT NULL,body TEXT NOT NULL,PRIMARY KEY(target,channel,sequence));
CREATE TABLE IF NOT EXISTS operations (id TEXT PRIMARY KEY,account TEXT NOT NULL,device TEXT NOT NULL,instance TEXT NOT NULL,release TEXT NOT NULL REFERENCES releases(id),status TEXT NOT NULL,receipt TEXT NOT NULL,created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS operation_events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,operation TEXT NOT NULL,receipt TEXT NOT NULL,created INTEGER NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS live_target ON operations(account,device,instance) WHERE status NOT IN ('applied','rolled_back','cancelled');
PRAGMA user_version=1;")?;
        Ok(Self {
            root: root.to_owned(),
            config,
            db,
        })
    }
    fn key(&self) -> Result<SigningKey> {
        let bytes: [u8; 32] = config::read_private_file(&self.root.join("signing-key"), 32)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid signing key"))?;
        Ok(SigningKey::from_bytes(&bytes))
    }
    pub fn public_key(&self) -> Result<String> {
        Ok(hex::encode(self.key()?.verifying_key().as_bytes()))
    }
    pub fn add_account(
        &self,
        account: &str,
        password: &str,
        passphrase: &str,
        otp_seed: Option<&str>,
    ) -> Result<()> {
        runtime::name(account)?;
        ensure!(
            (12..=4096).contains(&password.len()) && (12..=4096).contains(&passphrase.len()),
            "credentials require 12–4096 bytes"
        );
        if let Some(seed) = otp_seed {
            security::validate_seed(seed)?;
        }
        self.db.execute(
            "INSERT INTO accounts(name,password,passphrase,otp_seed) VALUES (?1,?2,?3,?4)",
            params![
                account,
                security::hash(password)?,
                security::hash(passphrase)?,
                otp_seed
            ],
        )?;
        Ok(())
    }
    pub fn plan(&self, id: &str, body: &Value) -> Result<()> {
        runtime::name(id)?;
        ensure!(serde_json::to_vec(body)?.len() <= 65536, "plan too large");
        self.db.execute(
            "INSERT INTO plans VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            params![id, body.to_string()],
        )?;
        Ok(())
    }
    pub fn grant(&self, account: &str, plan: &str) -> Result<()> {
        self.db.execute(
            "INSERT OR IGNORE INTO grants VALUES (?1,?2)",
            params![account, plan],
        )?;
        Ok(())
    }
    pub fn disable_account(&self, account: &str) -> Result<()> {
        self.db
            .execute("UPDATE accounts SET disabled=1 WHERE name=?1", [account])?;
        self.db
            .execute("DELETE FROM sessions WHERE account=?1", [account])?;
        Ok(())
    }
    pub fn publish_self(&self, release: crate::update::Release, binary: &Path) -> Result<String> {
        ensure!(
            release.schema == 1 && release.protocol == 1 && release.expires_at > runtime::now(),
            "invalid self release"
        );
        runtime::name(&release.target)?;
        runtime::name(&release.channel)?;
        semver::Version::parse(&release.version)?;
        let bytes = std::fs::read(binary)?;
        ensure!(
            bytes.len() <= 64 * 1024 * 1024
                && release.size == bytes.len() as u64
                && release.sha256 == runtime::digest(&bytes)
                && release.artifact == format!("/artifacts/{}", release.sha256),
            "self artifact mismatch"
        );
        crate::protocol::validate(
            &release.iteration,
            release.sequence,
            release.contract_epoch,
            &release.supports_contracts,
        )?;
        ensure!(
            release.iteration["artifact_sha256"] == release.sha256,
            "self iteration artifact mismatch"
        );
        let highest: i64 = self.db.query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM self_releases WHERE target=?1 AND channel=?2",
            params![release.target, release.channel],
            |r| r.get(0),
        )?;
        ensure!(
            release.sequence as i64 > highest,
            "self sequence must increase"
        );
        let signed = Signed::create(&release, &self.key()?)?;
        config::write_private_file(&self.root.join("artifacts").join(&release.sha256), &bytes)?;
        self.db.execute(
            "INSERT INTO self_releases VALUES (?1,?2,?3,?4,?5)",
            params![
                release.target,
                release.channel,
                release.sequence as i64,
                release.sha256,
                serde_json::to_string(&signed)?
            ],
        )?;
        signed.identity()
    }
    pub fn publish(&self, mut release: ProductRelease, directory: &Path) -> Result<String> {
        ensure!(
            release.host_id == self.config.host_id && release.expires_at > runtime::now(),
            "release host/expiry mismatch"
        );
        let base = directory.canonicalize()?;
        let mut files = Vec::new();
        fn visit(base: &Path, path: &Path, out: &mut Vec<ReleaseFile>, root: &Path) -> Result<()> {
            for item in std::fs::read_dir(path)? {
                let path = item?.path();
                let meta = std::fs::symlink_metadata(&path)?;
                ensure!(
                    !meta.file_type().is_symlink(),
                    "release links are unsupported"
                );
                if meta.is_dir() {
                    visit(base, &path, out, root)?;
                } else {
                    ensure!(
                        meta.is_file() && meta.len() <= 256 * 1024 * 1024,
                        "invalid artifact"
                    );
                    let name = path
                        .strip_prefix(base)?
                        .to_str()
                        .context("artifact path must be UTF-8")?
                        .replace('\\', "/");
                    runtime::relative_file(&name)?;
                    let data = std::fs::read(&path)?;
                    let hash = runtime::digest(&data);
                    let dest = root.join("artifacts").join(&hash);
                    if !dest.exists() {
                        config::write_private_file(&dest, &data)?;
                    }
                    #[cfg(unix)]
                    let executable = {
                        use std::os::unix::fs::PermissionsExt;
                        meta.permissions().mode() & 0o111 != 0
                    };
                    #[cfg(not(unix))]
                    let executable = name.ends_with(".exe");
                    out.push(ReleaseFile {
                        path: name,
                        sha256: hash,
                        size: data.len() as u64,
                        executable,
                    });
                    ensure!(out.len() <= 10000, "too many artifacts");
                }
            }
            Ok(())
        }
        visit(&base, &base, &mut files, &self.root)?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        release.files = files;
        release.validate()?;
        let highest: i64 = self.db.query_row("SELECT COALESCE(MAX(sequence),0) FROM releases WHERE plan=?1 AND product=?2 AND target=?3 AND channel=?4",params![release.plan,release.product,release.target,release.channel],|r|r.get(0))?;
        ensure!(
            release.sequence as i64 > highest,
            "release sequence must increase"
        );
        let signed = Signed::create(&release, &self.key()?)?;
        let id = signed.identity()?;
        self.db.execute(
            "INSERT INTO releases VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                release.plan,
                release.product,
                release.target,
                release.channel,
                release.sequence as i64,
                serde_json::to_string(&signed)?
            ],
        )?;
        Ok(id)
    }
    fn principal(&self, headers: &HeaderMap) -> Result<(String, String)> {
        let token = headers
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .context("unauthorized")?;
        ensure!(token.len() <= 256, "unauthorized");
        self.db.query_row("SELECT s.account,s.device FROM sessions s JOIN accounts a ON a.name=s.account WHERE s.hash=?1 AND s.expires>?2 AND a.disabled=0",params![runtime::digest(token.as_bytes()),runtime::now() as i64],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.context("unauthorized")
    }
    fn allowed(&self, account: &str, plan: &str) -> Result<()> {
        ensure!(
            self.db
                .query_row(
                    "SELECT 1 FROM grants WHERE account=?1 AND plan=?2",
                    params![account, plan],
                    |r| r.get::<_, u32>(0)
                )
                .optional()?
                .is_some(),
            "forbidden"
        );
        Ok(())
    }
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        query: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<(StatusCode, Value)> {
        if method == "GET" && path == "/health" {
            return Ok((
                StatusCode::OK,
                json!({"ok":true,"host_id":self.config.host_id,"version":crate::BUILD_VERSION,"schema":1}),
            ));
        }
        if method == "POST" && path == "/v1/login" {
            return self.login(body);
        }
        let (account, device) = self.principal(headers)?;
        let q: std::collections::HashMap<_, _> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        match (method, path) {
            ("GET", "/v1/releases/latest") => {
                self.allowed(&account, "gesac")?;
                let body:String=self.db.query_row("SELECT body FROM self_releases WHERE target=?1 AND channel=?2 ORDER BY sequence DESC LIMIT 1",params![q.get("target").context("target required")?,q.get("channel").map(String::as_str).unwrap_or("stable")],|r|r.get(0))?;
                Ok((StatusCode::OK, serde_json::from_str(&body)?))
            }
            ("POST", "/v1/logout") => {
                let token = headers["authorization"]
                    .to_str()?
                    .trim_start_matches("Bearer ");
                self.db.execute(
                    "DELETE FROM sessions WHERE hash=?1",
                    [runtime::digest(token.as_bytes())],
                )?;
                Ok((StatusCode::OK, json!({"revoked":true})))
            }
            ("GET", "/v1/plans") => {
                let mut st=self.db.prepare("SELECT p.id,p.body FROM plans p JOIN grants g ON g.plan=p.id WHERE g.account=?1 ORDER BY p.id")?;
                let rows = st.query_map([&account], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                let mut plans = Vec::new();
                for row in rows {
                    let (id, body) = row?;
                    plans.push(json!({"id":id,"definition":serde_json::from_str::<Value>(&body)?}));
                }
                Ok((
                    StatusCode::OK,
                    json!({"host_id":self.config.host_id,"plans":plans}),
                ))
            }
            ("GET", "/v1/products/latest") => {
                let plan = q.get("plan").context("plan required")?;
                self.allowed(&account, plan)?;
                let row:String=self.db.query_row("SELECT body FROM releases WHERE plan=?1 AND product=?2 AND target=?3 AND channel=?4 ORDER BY sequence DESC LIMIT 1",params![plan,q.get("product").context("product required")?,q.get("target").context("target required")?,q.get("channel").map(String::as_str).unwrap_or("stable")],|r|r.get(0))?;
                Ok((StatusCode::OK, serde_json::from_str(&row)?))
            }
            ("POST", "/v1/operations") => {
                self.reserve(&account, &device, serde_json::from_slice(body)?)
            }
            _ if path.starts_with("/v1/operations/") => {
                self.operation(&account, &device, method, &path[15..], body)
            }
            _ => anyhow::bail!("not found"),
        }
    }
    fn reserve(&mut self, account: &str, device: &str, body: Value) -> Result<(StatusCode, Value)> {
        let id = field(&body, "id")?;
        let instance = field(&body, "instance")?;
        let release = field(&body, "release")?;
        runtime::name(id)?;
        runtime::name(instance)?;
        let plan: String =
            self.db
                .query_row("SELECT plan FROM releases WHERE id=?1", [release], |r| {
                    r.get(0)
                })?;
        self.allowed(account, &plan)?;
        if let Some((a, d, i, r, status)) = self
            .db
            .query_row(
                "SELECT account,device,instance,release,status FROM operations WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
        {
            ensure!(
                (a.as_str(), d.as_str(), i.as_str(), r.as_str())
                    == (account, device, instance, release),
                "operation identity conflict"
            );
            return Ok((
                StatusCode::OK,
                json!({"id":id,"status":status,"duplicate":true}),
            ));
        }
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let busy:Option<String>=tx.query_row("SELECT id FROM operations WHERE account=?1 AND device=?2 AND instance=?3 AND status NOT IN ('applied','rolled_back','cancelled')",params![account,device,instance],|r|r.get(0)).optional()?;
        ensure!(busy.is_none(), "target has an unresolved operation");
        let redis = self
            .config
            .redis_url
            .as_deref()
            .context("online coordination requires Redis")?;
        coordination::claim(redis, &self.config.host_id, account, device, instance, id)?;
        tx.execute(
            "INSERT INTO operations VALUES (?1,?2,?3,?4,?5,'reserved','{}',?6)",
            params![
                id,
                account,
                device,
                instance,
                release,
                runtime::now() as i64
            ],
        )?;
        tx.commit()?;
        Ok((
            StatusCode::OK,
            json!({"id":id,"status":"reserved","duplicate":false}),
        ))
    }
    fn operation(
        &mut self,
        account: &str,
        device: &str,
        method: &str,
        id: &str,
        body: &[u8],
    ) -> Result<(StatusCode, Value)> {
        runtime::name(id)?;
        let (instance,status,receipt):(String,String,String)=self.db.query_row("SELECT instance,status,receipt FROM operations WHERE id=?1 AND account=?2 AND device=?3",params![id,account,device],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if method == "GET" {
            return Ok((
                StatusCode::OK,
                json!({"id":id,"status":status,"receipt":serde_json::from_str::<Value>(&receipt)?}),
            ));
        }
        ensure!(method == "POST", "unsupported method");
        let value: Value = serde_json::from_slice(body)?;
        let next = field(&value, "status")?;
        ensure!(
            [
                "prepared",
                "applying",
                "unknown",
                "applied",
                "rolled_back",
                "cancelled"
            ]
            .contains(&next),
            "invalid operation status"
        );
        let rollback = status == "applied" && next == "rolled_back";
        if rollback {
            let busy:u32=self.db.query_row("SELECT COUNT(*) FROM operations WHERE account=?1 AND device=?2 AND instance=?3 AND id<>?4 AND status NOT IN ('applied','rolled_back','cancelled')",params![account,device,instance,id],|r|r.get(0))?;
            ensure!(busy == 0, "another update owns this target");
        }
        if !rollback && ["applied", "rolled_back", "cancelled"].contains(&status.as_str()) {
            ensure!(
                status == next && serde_json::from_str::<Value>(&receipt)? == value,
                "terminal receipt conflict"
            );
        } else {
            ensure!(
                next != "cancelled" || status == "reserved" || status == "prepared",
                "executed operation requires reconciliation"
            );
            self.db.execute(
                "UPDATE operations SET status=?1,receipt=?2 WHERE id=?3",
                params![next, value.to_string(), id],
            )?;
        }
        self.db.execute(
            "INSERT INTO operation_events(operation,receipt,created) VALUES (?1,?2,?3)",
            params![id, value.to_string(), runtime::now() as i64],
        )?;
        if let Some(redis) = &self.config.redis_url {
            coordination::project(
                redis,
                &self.config.host_id,
                account,
                device,
                &instance,
                id,
                next,
            )?;
        }
        Ok((StatusCode::OK, json!({"id":id,"status":next})))
    }
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .context("missing string field")
}

async fn handle(
    State(service): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Response> {
        let mut s = service
            .lock()
            .map_err(|_| anyhow::anyhow!("service unavailable"))?;
        if method == Method::GET && uri.path().starts_with("/artifacts/") {
            let (account, _) = s.principal(&headers)?;
            let hash = &uri.path()[11..];
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid artifact"
            );
            let mut st = s.db.prepare(
                "SELECT r.body FROM releases r JOIN grants g ON g.plan=r.plan WHERE g.account=?1",
            )?;
            let mut permitted = s.allowed(&account, "gesac").is_ok()
                && s.db
                    .query_row(
                        "SELECT 1 FROM self_releases WHERE sha256=?1 LIMIT 1",
                        [hash],
                        |r| r.get::<_, u32>(0),
                    )
                    .optional()?
                    .is_some();
            for row in st.query_map([account], |r| r.get::<_, String>(0))? {
                let signed: Signed = serde_json::from_str(&row?)?;
                if signed
                    .verify(&s.public_key()?)?
                    .files
                    .iter()
                    .any(|f| f.sha256 == hash)
                {
                    permitted = true;
                    break;
                }
            }
            ensure!(permitted, "forbidden");
            let bytes =
                config::read_private_file(&s.root.join("artifacts").join(hash), 256 * 1024 * 1024)?;
            ensure!(
                runtime::digest(&bytes) == hash,
                "artifact integrity failure"
            );
            return Ok(([("content-type", "application/octet-stream")], bytes).into_response());
        }
        let (status, value) = s.request(
            method.as_str(),
            uri.path(),
            uri.query().unwrap_or(""),
            &headers,
            &body,
        )?;
        Ok((status, axum::Json(value)).into_response())
    })
    .await;
    match result {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => {
            let msg = e.to_string();
            let code = if msg == "unauthorized" {
                StatusCode::UNAUTHORIZED
            } else if msg == "forbidden" {
                StatusCode::FORBIDDEN
            } else if msg == "not found" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            };
            (
                code,
                axum::Json(json!({"error":"request rejected","code":code.as_u16()})),
            )
                .into_response()
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({"error":"service unavailable"})),
        )
            .into_response(),
    }
}
pub fn serve(root: &Path) -> Result<()> {
    let service = Service::open(root)?;
    let listen = service.config.listen;
    ensure!(
        listen.ip().is_loopback() || service.config.tls_cert.is_some(),
        "non-loopback service requires TLS"
    );
    let cert = service.config.tls_cert.clone();
    let key = service.config.tls_key.clone();
    let app = Router::new()
        .route("/", any(handle))
        .route("/{*path}", any(handle))
        .layer(DefaultBodyLimit::max(65536))
        .with_state(Arc::new(Mutex::new(service)));
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async move {
            if let (Some(cert), Some(key)) = (cert, key) {
                let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
                let handle = axum_server::Handle::new();
                let stop = handle.clone();
                tokio::spawn(async move {
                    shutdown_signal().await;
                    stop.graceful_shutdown(Some(std::time::Duration::from_secs(15)));
                });
                axum_server::bind_rustls(listen, tls)
                    .handle(handle)
                    .serve(app.into_make_service())
                    .await?;
            } else {
                let listener = tokio::net::TcpListener::bind(listen).await?;
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown_signal())
                    .await?;
            }
            Ok::<(), anyhow::Error>(())
        })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// The service supervisor owns only its server child; it observes committed self-update selection.
pub fn supervise(root: &Path, home: &Path) -> Result<()> {
    use std::{
        process::{Command, Stdio},
        thread,
        time::{Duration, Instant},
    };
    let mut selected = crate::update::selected_core(home)?;
    let start = |path: &Path| {
        Command::new(path)
            .arg("server")
            .arg("serve")
            .arg("--root")
            .arg(root)
            .stdin(Stdio::null())
            .spawn()
    };
    let mut child = start(&selected)?;
    loop {
        thread::sleep(Duration::from_secs(2));
        let next = crate::update::selected_core(home)?;
        let exited = child.try_wait()?.is_some();
        if !exited && next == selected {
            continue;
        }
        if !exited {
            #[cfg(unix)]
            {
                let _ = Command::new("/bin/kill")
                    .args(["-TERM", &child.id().to_string()])
                    .status();
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
            let deadline = Instant::now() + Duration::from_secs(20);
            while child.try_wait()?.is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(100));
            }
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
            child.wait()?;
        }
        selected = next;
        child = start(&selected)?;
    }
}
