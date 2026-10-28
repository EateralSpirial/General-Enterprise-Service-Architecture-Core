use crate::{
    model::{digest, now, Release, Signed},
    server::Service,
};
use anyhow::Result;
use clap::Subcommand;
use rusqlite::params;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Subcommand)]
pub enum Command {
    Account {
        name: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        password_stdin: bool,
    },
    Disable {
        account: String,
    },
    Password {
        account: String,
        #[arg(long)]
        password_stdin: bool,
    },
    Product {
        id: String,
        #[arg(long)]
        label: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Grant {
        account: String,
        product: String,
        #[arg(long, default_value = "*")]
        device: String,
    },
    Revoke {
        account: String,
        product: String,
        #[arg(long, default_value = "*")]
        device: String,
    },
    Publish {
        product: String,
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "windows-x86_64")]
        target: String,
        #[arg(long, value_parser = ["zip", "msi", "exe"])] // GATE
        format: String,
        #[arg(long)]
        artifact: PathBuf,
        #[arg(long)]
        entrypoint: Option<String>,
        #[arg(long = "arg", allow_hyphen_values = true)]
        arguments: Vec<String>,
        #[arg(long, default_value = "user", value_parser = ["user", "machine"])] // GATE
        scope: String,
    },
    Client {
        #[arg(long)]
        target: String,
        #[arg(long)]
        file: PathBuf,
    },
    Requests,
    Devices,
    Accounts,
    Key,
}

fn read_password(stdin: bool) -> Result<String> // GATE
{
    let value = if stdin {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        line.trim_end_matches(['\n', '\r']).to_owned()
    } else {
        rpassword::prompt_password("Password: ")?
    };
    anyhow::ensure!(
        value.len() >= 12,
        "Use a company password with at least 12 bytes"
    ); // GATE
    Ok(value)
}

pub fn run(root: &Path, command: Command) -> Result<()> {
    let service = Service::open(root, "https://yanhan.chat/gesa/api/")?;
    let db = service.db()?;
    let result = match command {
        Command::Account {
            name,
            label,
            password_stdin,
        } => {
            let password = super::server::password_hash(&read_password(password_stdin)?)?; // GATE
            db.execute(
                "INSERT INTO accounts VALUES (?1,?2,?3,1,?4)",
                params![name, label, password, now()],
            )?;
            json!({"account":name,"label":label})
        }
        Command::Disable { account } => {
            db.execute("UPDATE accounts SET enabled=0 WHERE id=?1", [&account])?; // GATE
            db.execute("DELETE FROM sessions WHERE account=?1", [&account])?;
            json!({"disabled":account})
        }
        Command::Password {
            account,
            password_stdin,
        } => {
            let password = super::server::password_hash(&read_password(password_stdin)?)?; // GATE
            db.execute(
                "UPDATE accounts SET password=?1 WHERE id=?2",
                params![password, account],
            )?;
            db.execute("DELETE FROM sessions WHERE account=?1", [&account])?;
            json!({"password_updated":account,"sessions_revoked":true})
        }
        Command::Product {
            id,
            label,
            description,
        } => {
            db.execute("INSERT INTO products VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET label=excluded.label,description=excluded.description", params![id,label,description])?;
            json!({"product":id,"label":label})
        }
        Command::Grant {
            account,
            product,
            device,
        } => {
            db.execute(
                "INSERT OR IGNORE INTO grants VALUES (?1,?2,?3)",
                params![account, product, device],
            )?;
            json!({"account":account,"product":product,"device":device,"authorized":true})
        }
        Command::Revoke {
            account,
            product,
            device,
        } => {
            db.execute(
                "DELETE FROM grants WHERE account=?1 AND product=?2 AND device=?3",
                params![account, product, device],
            )?; // GATE
            json!({"account":account,"product":product,"device":device,"authorized":false})
        }
        Command::Publish {
            product,
            version,
            target,
            format,
            artifact,
            entrypoint,
            arguments,
            scope,
        } => {
            let bytes = fs::read(artifact)?;
            let sha = digest(&bytes);
            let sequence: i64 = db.query_row(
                "SELECT COALESCE(MAX(sequence),0)+1 FROM releases WHERE product=?1 AND target=?2",
                params![product, target],
                |r| r.get(0),
            )?;
            let release = Release {
                schema: 1,
                product,
                version,
                target,
                sequence,
                sha256: sha.clone(),
                size: bytes.len() as u64,
                format,
                entrypoint,
                arguments,
                scope,
                created: now(),
            };
            let signed = Signed::create(&release, &service.key()?)?;
            fs::write(root.join("artifacts").join(&sha), bytes)?;
            db.execute(
                "INSERT INTO releases VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    release.product,
                    release.target,
                    sequence,
                    sha,
                    release.version,
                    serde_json::to_string(&signed)?
                ],
            )?;
            json!({"release":release,"signature":signed.signature})
        }
        Command::Client { target, file } => {
            let bytes = fs::read(file)?;
            let sha = digest(&bytes);
            fs::write(root.join("clients").join(&sha), &bytes)?;
            db.execute("INSERT INTO clients VALUES (?1,?2,?3,?4) ON CONFLICT(target) DO UPDATE SET sha256=excluded.sha256,size=excluded.size,version=excluded.version", params![target,sha,bytes.len() as i64,env!("CARGO_PKG_VERSION")])?;
            json!({"target":target,"sha256":sha,"size":bytes.len()})
        }
        Command::Requests => {
            let mut st = db.prepare("SELECT s.account,s.product,s.device,s.created,EXISTS(SELECT 1 FROM grants g WHERE g.account=s.account AND g.product=s.product AND (g.device='*' OR g.device=s.device)) FROM selections s ORDER BY s.created DESC")?;
            let rows = st.query_map([], |r| Ok(json!({"account":r.get::<_,String>(0)?,"product":r.get::<_,String>(1)?,"device":r.get::<_,String>(2)?,"created":r.get::<_,i64>(3)?,"authorized":r.get::<_,bool>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            json!({"requests":rows})
        }
        Command::Devices => {
            let mut st = db.prepare(
                "SELECT account,id,name,os,arch,last_seen FROM devices ORDER BY last_seen DESC",
            )?;
            let rows = st.query_map([], |r| Ok(json!({"account":r.get::<_,String>(0)?,"id":r.get::<_,String>(1)?,"name":r.get::<_,String>(2)?,"os":r.get::<_,String>(3)?,"arch":r.get::<_,String>(4)?,"last_seen":r.get::<_,i64>(5)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            json!({"devices":rows})
        }
        Command::Accounts => {
            let mut st =
                db.prepare("SELECT id,label,enabled,created FROM accounts ORDER BY label")?;
            let rows = st.query_map([], |r| Ok(json!({"id":r.get::<_,String>(0)?,"label":r.get::<_,String>(1)?,"enabled":r.get::<_,bool>(2)?,"created":r.get::<_,i64>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            json!({"accounts":rows})
        }
        Command::Key => {
            json!({"release_public_key":hex::encode(service.key()?.verifying_key().to_bytes())})
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
