use super::*;
use crate::model::{id, Login, Session};
use scrypt::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Scrypt,
};

pub fn password_hash(password: &str) -> Result<String> // GATE: credential protection
{
    Ok(Scrypt
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))?
        .to_string()) // GATE
}

fn rejected() -> Error // GATE
{
    Error::new(
        401,
        "credentials_rejected",
        "Check the company account and password with the publisher",
    )
}

pub async fn login(
    State(service): State<Arc<Service>>,
    Json(input): Json<Login>,
) -> ApiResult<Response> // GATE: authenticate before enrollment
{
    let db = service.db()?;
    let hash: String = db
        .query_row(
            "SELECT password FROM accounts WHERE id=?1 AND enabled=1",
            [&input.account],
            |r| r.get(0),
        )
        .map_err(|_| rejected())?; // GATE
    Scrypt
        .verify_password(input.password.as_bytes(), &PasswordHash::new(&hash)?)
        .map_err(|_| rejected())?; // GATE
    let device = input.device.as_ref().map(|d| d.id.clone());
    if let Some(d) = input.device {
        devices::save_device(&db, &input.account, &d)?;
    }
    let session = Session {
        token: format!("{}{}", id(), id()),
        account: input.account,
        expires_at: now()
            + if device.is_some() {
                30 * 86400
            } else {
                12 * 3600
            },
    };
    db.execute(
        "INSERT INTO sessions VALUES (?1,?2,?3,?4)",
        rusqlite::params![
            digest(session.token.as_bytes()),
            session.account,
            device,
            session.expires_at
        ],
    )?;
    let cookie = format!(
        "gesa_session={}; HttpOnly; Secure; SameSite=Strict; Path=/gesa/; Max-Age=43200",
        session.token
    ); // GATE: browser session protection
    Ok((
        [("set-cookie", cookie), ("cache-control", "no-store".into())],
        Json(session),
    )
        .into_response())
}

pub async fn current(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Json<Value>> {
    let label: String = service.db()?.query_row(
        "SELECT label FROM accounts WHERE id=?1",
        [&p.account],
        |r| r.get(0),
    )?;
    Ok(Json(
        json!({"account":p.account,"label":label,"device":p.device}),
    ))
}

pub async fn logout(
    State(service): State<Arc<Service>>,
    Extension(p): Extension<Principal>,
) -> ApiResult<Response> {
    service.db()?.execute(
        "DELETE FROM sessions WHERE hash=?1 AND account=?2",
        rusqlite::params![p.session_hash, p.account],
    )?;
    Ok((
        [(
            "set-cookie",
            "gesa_session=; HttpOnly; Secure; SameSite=Strict; Path=/gesa/; Max-Age=0",
        )],
        Json(json!({"logged_out":true})),
    )
        .into_response())
}

pub async fn authenticate(
    State(service): State<Arc<Service>>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> ApiResult<Response> // GATE
{
    let principal = service.principal(request.headers())?; // GATE
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}
