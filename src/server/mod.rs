mod auth;
mod catalog;
mod devices;
pub use auth::password_hash;

use crate::model::{digest, now};
use anyhow::Result;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Service {
    pub root: PathBuf,
    pub public_url: String,
}

impl Service {
    pub fn open(root: &Path, public_url: &str) -> Result<Self> // GATE
    {
        fs::create_dir_all(root)?;
        #[cfg(unix)] // GATE: private state before creating credentials
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?; // GATE
        }
        fs::create_dir_all(root.join("artifacts"))?;
        fs::create_dir_all(root.join("clients"))?;
        let service = Self {
            root: root.to_owned(),
            public_url: public_url.trim_end_matches('/').to_owned(),
        };
        let db = service.db()?;
        db.execute_batch(include_str!("schema.sql"))?;
        let key = root.join("signing.key");
        if !key.exists() {
            fs::write(&key, rand::random::<[u8; 32]>())?;
        }
        Ok(service)
    }

    pub fn db(&self) -> Result<Connection> // GATE
    {
        let db = Connection::open(self.root.join("enterprise.sqlite"))?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        Ok(db)
    }

    pub fn key(&self) -> Result<SigningKey> // GATE
    {
        let bytes = fs::read(self.root.join("signing.key"))?;
        Ok(SigningKey::from_bytes(bytes.as_slice().try_into()?))
    }

    pub fn principal(&self, headers: &HeaderMap) -> ApiResult<Principal> // GATE: account and device authority
    {
        let token = headers
            .get("authorization")
            .and_then(|s| s.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .or_else(|| {
                headers
                    .get("cookie")
                    .and_then(|s| s.to_str().ok())
                    .and_then(|s| s.split("; ").find_map(|v| v.strip_prefix("gesa_session=")))
            })
            .ok_or_else(|| Error::new(401, "login_required", "Log in with your company account"))?; // GATE
        let db = self.db()?;
        let session_hash = digest(token.as_bytes());
        let p = db.query_row("SELECT s.account,s.device FROM sessions s JOIN accounts a ON a.id=s.account WHERE s.hash=?1 AND s.expires>?2 AND a.enabled=1", rusqlite::params![session_hash, now()], |r| Ok(Principal { account: r.get(0)?, device: r.get(1)?, session_hash: session_hash.clone() })).map_err(|_| Error::new(401, "session_expired", "Log in again to renew this session"))?; // GATE
        Ok(p)
    }

    pub fn permitted(&self, p: &Principal, product: &str, device: &str) -> ApiResult<()> // GATE: product entitlement
    {
        let allowed: bool = self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM grants WHERE account=?1 AND product=?2 AND (device='*' OR device=?3))", rusqlite::params![p.account, product, device], |r| r.get(0))?; // GATE
        allowed.then_some(()).ok_or_else(|| {
            Error::new(
                403,
                "software_permission_required",
                "Select this software in the portal and ask the publisher to open its permission",
            )
        }) // GATE
    }
}

#[derive(Clone)]
pub struct Principal {
    pub account: String,
    pub device: Option<String>,
    pub session_hash: String,
}

impl Principal {
    pub fn device(&self) -> ApiResult<&str> // GATE: device operations need CLI enrollment
    {
        self.device.as_deref().ok_or_else(|| {
            Error::new(
                403,
                "device_login_required",
                "Run gesac login on the target computer",
            )
        }) // GATE
    }
}

pub struct Error(Box<Response>);
pub type ApiResult<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(status: u16, code: &str, recovery: &str) -> Self // GATE: common error response
    {
        Self(Box::new(
            (
                StatusCode::from_u16(status).unwrap(),
                Json(json!({"error":code,"recovery":recovery})),
            )
                .into_response(),
        ))
    }
}
impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(value: E) -> Self // GATE: unified native failure conversion
    {
        let error = value.into();
        eprintln!("enterprise request: {error}");
        Self::new(
            409,
            "operation_unavailable",
            "Refresh the operation status and contact the publisher if the condition persists",
        ) // GATE
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response // GATE: expose reason and recovery
    {
        *self.0
    }
}

async fn health(State(service): State<Arc<Service>>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"protocol":"gesa-enterprise/1","release_public_key":hex::encode(service.key()?.verifying_key().to_bytes())}),
    ))
}

pub fn router(service: Service) -> Router // GATE
{
    let service = Arc::new(service);
    let protected = Router::new()
        .route("/session", get(auth::current).delete(auth::logout))
        .route("/catalog", get(catalog::list))
        .route(
            "/selections",
            get(catalog::selections).post(catalog::select),
        )
        .route("/devices", get(devices::list))
        .route("/devices/register", post(devices::register))
        .route("/devices/{id}", get(devices::detail))
        .route(
            "/deployments",
            get(devices::deployments).post(devices::reserve),
        )
        .route("/deployments/{id}/result", post(devices::report))
        .route("/releases/{product}/latest", get(catalog::latest))
        .route("/artifacts/{sha}", get(catalog::artifact))
        .route("/downloads", get(catalog::downloads))
        .route("/downloads/{target}", get(catalog::client))
        .route_layer(middleware::from_fn_with_state(
            service.clone(),
            auth::authenticate,
        )); // GATE: shared authenticated route boundary
    Router::new()
        .route("/health", get(health))
        .route("/session/login", post(auth::login))
        .merge(protected)
        .with_state(service)
}

pub fn serve(root: PathBuf, listen: String, public_url: String) -> Result<()> // GATE
{
    let service = Service::open(&root, &public_url)?;
    let address: std::net::SocketAddr = listen.parse()?;
    anyhow::ensure!(
        address.ip().is_loopback(),
        "Bind the service to loopback and expose it through the HTTPS gateway"
    ); // GATE
    tokio::runtime::Runtime::new()?.block_on(async {
        let listener = tokio::net::TcpListener::bind(address).await?;
        axum::serve(listener, router(service))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        Ok(())
    })
}
