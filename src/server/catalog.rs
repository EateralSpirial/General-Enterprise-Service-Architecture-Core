use super::*;
use axum::{
    extract::{Path as Route, Query},
    http::header,
};
use rusqlite::params;
use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct Filter {
    pub device: Option<String>,
    pub target: Option<String>,
}

pub async fn list(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Query(query): Query<Filter>,
) -> ApiResult<Json<Value>> {
    let db = service.db()?;
    let device = p
        .device
        .as_deref()
        .or(query.device.as_deref())
        .unwrap_or("*");
    let mut statement = db.prepare("SELECT p.id,p.label,p.description,EXISTS(SELECT 1 FROM grants g WHERE g.account=?1 AND g.product=p.id AND (g.device='*' OR g.device=?2)),EXISTS(SELECT 1 FROM selections s WHERE s.account=?1 AND s.product=p.id AND s.device=?2) FROM products p ORDER BY p.label")?; // GATE: scoped permissions
    let products = statement.query_map(params![p.account, device], |r| Ok(json!({"id":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?,"description":r.get::<_,String>(2)?,"authorized":r.get::<_,bool>(3)?,"selected":r.get::<_,bool>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut releases = db.prepare("SELECT target,version,sequence FROM releases WHERE product=?1 ORDER BY target,sequence DESC")?;
    let mut values = Vec::new();
    for mut product in products {
        let rows = releases.query_map([product["id"].as_str().unwrap()], |r| Ok(json!({"target":r.get::<_,String>(0)?,"version":r.get::<_,String>(1)?,"sequence":r.get::<_,i64>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut targets = std::collections::HashSet::new();
        product["releases"] = json!(rows
            .into_iter()
            .filter(|r| targets.insert(r["target"].as_str().unwrap().to_owned()))
            .collect::<Vec<_>>());
        values.push(product);
    }
    Ok(Json(json!({"products":values,"device":device})))
}

#[derive(Deserialize)]
pub struct Selection {
    pub product: String,
    pub device: String,
}

pub async fn select(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Json(input): Json<Selection>,
) -> ApiResult<Json<Value>> {
    let db = service.db()?;
    db.query_row(
        "SELECT id FROM devices WHERE account=?1 AND id=?2",
        params![p.account, input.device],
        |r| r.get::<_, String>(0),
    )?; // GATE: choose an enrolled device
    db.execute(
        "INSERT OR IGNORE INTO selections VALUES (?1,?2,?3,?4)",
        params![p.account, input.product, input.device, now()],
    )?;
    let authorized = service.permitted(&p, &input.product, &input.device).is_ok();
    Ok(Json(
        json!({"product":input.product,"device":input.device,"status":if authorized {"ready"} else {"permission_requested"}}),
    ))
}

pub async fn selections(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let db = service.db()?;
    let mut st = db.prepare("SELECT s.product,p.label,s.device,s.created,EXISTS(SELECT 1 FROM grants g WHERE g.account=s.account AND g.product=s.product AND (g.device='*' OR g.device=s.device)) FROM selections s JOIN products p ON p.id=s.product WHERE s.account=?1 ORDER BY s.created DESC")?; // GATE: account ownership
    let rows = st.query_map([p.account], |r| Ok(json!({"product":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?,"device":r.get::<_,String>(2)?,"created":r.get::<_,i64>(3)?,"authorized":r.get::<_,bool>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Json(json!({"selections":rows})))
}

pub async fn latest(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Route(product): Route<String>,
    Query(filter): Query<Filter>,
) -> ApiResult<Json<Value>> {
    service.permitted(
        &p,
        &product,
        p.device
            .as_deref()
            .or(filter.device.as_deref())
            .unwrap_or("*"),
    )?; // GATE
    let body: String = service.db()?.query_row(
        "SELECT body FROM releases WHERE product=?1 AND target=?2 ORDER BY sequence DESC LIMIT 1",
        params![
            product,
            filter.target.unwrap_or_else(|| "windows-x86_64".into())
        ],
        |r| r.get(0),
    )?;
    Ok(Json(serde_json::from_str(&body)?))
}

pub async fn artifact(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Route(sha): Route<String>,
) -> ApiResult<Response> {
    let db = service.db()?;
    let product: String = db.query_row(
        "SELECT product FROM releases WHERE sha256=?1 LIMIT 1",
        [&sha],
        |r| r.get(0),
    )?;
    service.permitted(&p, &product, p.device.as_deref().unwrap_or("*"))?; // GATE: artifact authorization
    let bytes = fs::read(service.root.join("artifacts").join(&sha))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "private, no-store"),
            (header::CONTENT_DISPOSITION, "attachment"),
        ],
        bytes,
    )
        .into_response())
}

pub async fn downloads(
    State(service): State<Arc<Service>>,
    Extension(_p): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let db = service.db()?;
    let mut st = db.prepare("SELECT target,sha256,size,version FROM clients ORDER BY target")?;
    let rows = st.query_map([], |r| {
        let target: String = r.get(0)?;
        Ok(json!({"target":target,"sha256":r.get::<_,String>(1)?,"size":r.get::<_,i64>(2)?,"version":r.get::<_,String>(3)?,"url":format!("{}/downloads/{}",service.public_url,target)}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Json(
        json!({"downloads":rows,"host":service.public_url,"release_public_key":hex::encode(service.key()?.verifying_key().to_bytes())}),
    ))
}

pub async fn client(
    State(service): State<Arc<Service>>,
    Extension(_p): Extension<Principal>,
    Route(target): Route<String>,
) -> ApiResult<Response> {
    let sha: String = service.db()?.query_row(
        "SELECT sha256 FROM clients WHERE target=?1",
        [target],
        |r| r.get(0),
    )?;
    let bytes = fs::read(service.root.join("clients").join(sha))?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"gesac.exe\"",
            ),
            (header::CACHE_CONTROL, "private, no-store"),
        ],
        bytes,
    )
        .into_response())
}
