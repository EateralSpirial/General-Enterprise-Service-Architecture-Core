use super::*;
use crate::model::{id, Device};
use axum::extract::Path as Route;
use rusqlite::params;
use serde::Deserialize;

pub fn save_device(db: &Connection, account: &str, device: &Device) -> Result<()> {
    db.execute("INSERT INTO devices VALUES (?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(account,id) DO UPDATE SET name=excluded.name,os=excluded.os,arch=excluded.arch,last_seen=excluded.last_seen", params![account,device.id,device.name,device.os,device.arch,now()])?;
    Ok(())
}

pub async fn register(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Json(device): Json<Device>,
) -> ApiResult<Json<Value>> {
    (p.device()? == device.id)
        .then_some(())
        .ok_or_else(|| Error::new(403, "device_mismatch", "Log in again from this device"))?; // GATE
    save_device(&service.db()?, &p.account, &device)?;
    service.db()?.execute(
        "UPDATE sessions SET expires=?1 WHERE account=?2 AND device=?3",
        params![now() + 30 * 86400, p.account, device.id],
    )?;
    Ok(Json(
        json!({"registered":true,"device":device,"last_seen":now(),"expires_at":now()+30*86400}),
    ))
}

pub async fn list(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let db = service.db()?;
    let mut st = db.prepare("SELECT id,name,os,arch,first_seen,last_seen FROM devices WHERE account=?1 ORDER BY last_seen DESC")?; // GATE: account device boundary
    let rows = st.query_map([p.account], |r| Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"os":r.get::<_,String>(2)?,"arch":r.get::<_,String>(3)?,"first_seen":r.get::<_,i64>(4)?,"last_seen":r.get::<_,i64>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Json(json!({"devices":rows})))
}

pub async fn detail(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Route(device): Route<String>,
) -> ApiResult<Json<Value>> {
    let db = service.db()?;
    let info = db.query_row("SELECT name,os,arch,first_seen,last_seen FROM devices WHERE account=?1 AND id=?2", params![p.account,device], |r| Ok(json!({"id":device,"name":r.get::<_,String>(0)?,"os":r.get::<_,String>(1)?,"arch":r.get::<_,String>(2)?,"first_seen":r.get::<_,i64>(3)?,"last_seen":r.get::<_,i64>(4)?})))?; // GATE: account device boundary
    Ok(Json(
        json!({"device":info,"deployments":deployment_rows(&db,&p.account,Some(&device))?}),
    ))
}

fn deployment_rows(db: &Connection, account: &str, device: Option<&str>) -> Result<Vec<Value>> {
    let mut st = db.prepare("SELECT id,device,product,sequence,version,status,detail,created,updated FROM deployments WHERE account=?1 AND (?2 IS NULL OR device=?2) ORDER BY updated DESC")?; // GATE: account receipts
    let rows = st.query_map(params![account,device], |r| {
        let body: String = r.get(6)?;
        Ok(json!({"id":r.get::<_,String>(0)?,"device":r.get::<_,String>(1)?,"product":r.get::<_,String>(2)?,"sequence":r.get::<_,i64>(3)?,"version":r.get::<_,String>(4)?,"status":r.get::<_,String>(5)?,"detail":serde_json::from_str::<Value>(&body).unwrap_or(Value::Null),"created":r.get::<_,i64>(7)?,"updated":r.get::<_,i64>(8)?}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub async fn deployments(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"deployments":deployment_rows(&service.db()?,&p.account,p.device.as_deref())?}),
    ))
}

#[derive(Deserialize)]
pub struct Deployment {
    pub product: String,
}

pub async fn reserve(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Json(input): Json<Deployment>,
) -> ApiResult<Json<Value>> {
    let device = p.device()?; // GATE
    service.permitted(&p, &input.product, device)?; // GATE
    let db = service.db()?;
    let target: String = db.query_row(
        "SELECT os||'-'||arch FROM devices WHERE account=?1 AND id=?2",
        params![p.account, device],
        |r| r.get(0),
    )?;
    let (body, sequence, version): (String, i64, String) = db.query_row(
        "SELECT body,sequence,version FROM releases WHERE product=?1 AND target=?2 ORDER BY sequence DESC LIMIT 1",
        params![input.product, target],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    )?;
    let signed: Value = serde_json::from_str(&body)?;
    let operation = id();
    db.execute("INSERT OR IGNORE INTO deployments(id,account,device,product,sequence,version,status,created,updated) VALUES (?1,?2,?3,?4,?5,?6,'reserved',?7,?7)", params![operation,p.account,device,input.product,sequence,version,now()])?;
    let (operation,status): (String,String) = db.query_row("SELECT id,status FROM deployments WHERE account=?1 AND device=?2 AND product=?3 AND sequence=?4", params![p.account,device,input.product,sequence], |r| Ok((r.get(0)?,r.get(1)?)))?;
    Ok(Json(
        json!({"operation":operation,"status":status,"release":signed}),
    ))
}

#[derive(Deserialize)]
pub struct Report {
    pub status: String,
    pub detail: Value,
}

pub async fn report(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
    Route(operation): Route<String>,
    Json(report): Json<Report>,
) -> ApiResult<Json<Value>> {
    let device = p.device()?; // GATE
    ["installing", "installed", "failed", "restart_required"]
        .contains(&report.status.as_str())
        .then_some(())
        .ok_or_else(|| {
            Error::new(
                422,
                "invalid_status",
                "Use installing, installed, failed or restart_required",
            )
        })?; // GATE
    let count = service.db()?.execute("UPDATE deployments SET status=?1,detail=?2,updated=?3 WHERE id=?4 AND account=?5 AND device=?6", params![report.status,report.detail.to_string(),now(),operation,p.account,device])?; // GATE: device owns its receipt
    (count != 0).then_some(()).ok_or_else(|| {
        Error::new(
            404,
            "operation_missing",
            "Refresh the deployment list for this device",
        )
    })?; // GATE
    Ok(Json(json!({"operation":operation,"status":report.status})))
}
