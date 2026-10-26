//! Runtime CLI. Secrets enter through a bounded stdin document or hidden prompts.
use crate::{
    config::{self, Credentials},
    node::{Instance, Node, Profile},
    runtime,
    server::{ServerConfig, Service},
};
use anyhow::{ensure, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Subcommand)]
pub enum ServerCommand {
    Init {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "127.0.0.1:4180")]
        listen: std::net::SocketAddr,
        #[arg(long)]
        redis_config: Option<PathBuf>,
        #[arg(long)]
        tls_cert: Option<PathBuf>,
        #[arg(long)]
        tls_key: Option<PathBuf>,
    },
    Serve {
        #[arg(long)]
        root: PathBuf,
    },
    Supervise {
        #[arg(long)]
        root: PathBuf,
    },
    AccountAdd {
        #[arg(long)]
        root: PathBuf,
    },
    AccountDisable {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        account: String,
    },
    Plan {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        file: PathBuf,
    },
    Grant {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        account: String,
        #[arg(long)]
        plan: String,
    },
    Publish {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        directory: PathBuf,
    },
    PublishSelf {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        binary: PathBuf,
    },
    Status {
        #[arg(long)]
        root: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum NodeCommand {
    Sync,
    Watch {
        #[arg(long,default_value_t=60,value_parser=clap::value_parser!(u64).range(5..=86400))]
        interval: u64,
    },
    Init,
    Status,
    Profiles,
    SelfSource {
        #[arg(long)]
        profile: String,
    },
    Operation {
        #[arg(long)]
        id: String,
    },
    ProfileAdd {
        #[arg(long)]
        file: PathBuf,
    },
    ProfileRemove {
        #[arg(long)]
        id: String,
    },
    Login {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        otp: bool,
    },
    Plans {
        #[arg(long)]
        profile: String,
    },
    Subscribe {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        plan: String,
    },
    Register {
        #[arg(long)]
        file: PathBuf,
    },
    Check {
        #[arg(long)]
        instance: String,
    },
    Stage {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        offline: Option<PathBuf>,
    },
    ResumeStage {
        #[arg(long)]
        operation: String,
        #[arg(long)]
        offline: Option<PathBuf>,
    },
    Prepare {
        #[arg(long)]
        operation: String,
    },
    Apply {
        #[arg(long)]
        operation: String,
    },
    Reconcile {
        #[arg(long)]
        operation: String,
    },
    Rollback {
        #[arg(long)]
        operation: String,
    },
    Verify {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        version: Option<String>,
    },
    Export {
        #[arg(long)]
        operation: String,
        #[arg(long)]
        destination: PathBuf,
    },
    Uninstall,
}
pub fn input() -> Result<Value> {
    let mut bytes = Vec::new();
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 65536, "input too large");
    Ok(serde_json::from_slice(&bytes)?)
}
fn file(path: &Path) -> Result<Value> {
    let meta = std::fs::metadata(path)?;
    ensure!(meta.len() <= 4 * 1024 * 1024, "input file too large");
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn server(command: ServerCommand) -> Result<Value> {
    match command {
        ServerCommand::Init {
            root,
            listen,
            redis_config,
            tls_cert,
            tls_key,
        } => {
            let value = input()?;
            let redis_url = if let Some(path) = redis_config {
                Some(
                    serde_json::from_slice::<Value>(&config::read_private_file(&path, 16384)?)?
                        ["url"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Redis URL missing"))?
                        .to_owned(),
                )
            } else {
                None
            };
            let s = Service::init(
                &root,
                ServerConfig {
                    host_id: format!("host-{}", runtime::random_id()),
                    listen,
                    redis_url,
                    tls_cert,
                    tls_key,
                },
            )?;
            add_account(&s, &value)?;
            Ok(json!({"host_id":s.config.host_id,"public_key":s.public_key()?,"listen":listen}))
        }
        ServerCommand::Serve { root } => {
            crate::server::serve(&root)?;
            Ok(json!({"stopped":true}))
        }
        ServerCommand::Supervise { root } => {
            crate::server::supervise(&root, &config::home_dir()?)?;
            Ok(json!({"stopped":true}))
        }
        ServerCommand::AccountAdd { root } => {
            add_account(&Service::open(&root)?, &input()?)?;
            Ok(json!({"created":true}))
        }
        ServerCommand::AccountDisable { root, account } => {
            Service::open(&root)?.disable_account(&account)?;
            Ok(json!({"disabled":true}))
        }
        ServerCommand::Plan {
            root,
            id,
            file: path,
        } => {
            Service::open(&root)?.plan(&id, &file(&path)?)?;
            Ok(json!({"plan":id}))
        }
        ServerCommand::Grant {
            root,
            account,
            plan,
        } => {
            Service::open(&root)?.grant(&account, &plan)?;
            Ok(json!({"granted":true}))
        }
        ServerCommand::Publish {
            root,
            file: path,
            directory,
        } => {
            let id =
                Service::open(&root)?.publish(serde_json::from_value(file(&path)?)?, &directory)?;
            Ok(json!({"release_id":id}))
        }
        ServerCommand::PublishSelf {
            root,
            file: path,
            binary,
        } => {
            let id = Service::open(&root)?
                .publish_self(serde_json::from_value(file(&path)?)?, &binary)?;
            Ok(json!({"release_id":id}))
        }
        ServerCommand::Status { root } => {
            let s = Service::open(&root)?;
            Ok(
                json!({"host_id":s.config.host_id,"public_key":s.public_key()?,"listen":s.config.listen,"redis_configured":s.config.redis_url.is_some()}),
            )
        }
    }
}
fn add_account(service: &Service, value: &Value) -> Result<()> {
    service.add_account(
        value["account"].as_str().unwrap_or(""),
        value["password"].as_str().unwrap_or(""),
        value["passphrase"].as_str().unwrap_or(""),
        value["otp_seed"].as_str(),
    )
}
pub fn node(command: NodeCommand, root: &Path) -> Result<Value> {
    if let NodeCommand::Watch { interval } = &command {
        loop {
            let result = Node::open(root).and_then(|n| n.synchronize());
            println!(
                "{}",
                json!({"schema":1,"ok":result.is_ok(),"command":"node.watch","result":result.unwrap_or_else(|_|json!({"status":"needs_reconciliation"}))})
            );
            std::thread::sleep(std::time::Duration::from_secs(*interval));
        }
    }
    let node = if matches!(
        &command,
        NodeCommand::Status
            | NodeCommand::Profiles
            | NodeCommand::Operation { .. }
            | NodeCommand::Verify { .. }
    ) {
        Node::read(root)?
    } else {
        Node::open(root)?
    };
    match command {
        NodeCommand::Sync => node.synchronize(),
        NodeCommand::Watch { .. } => unreachable!(),
        NodeCommand::Init => Ok(json!({"device":node.device,"initialized":true})),
        NodeCommand::Status => node.status(),
        NodeCommand::Profiles => node.profiles(),
        NodeCommand::SelfSource { profile } => {
            let p = node.profile(&profile)?;
            let c = config::load_credentials(&node.profile_home(&profile)?, &p.connection)?;
            config::write_private_file(
                &root.join("config.json"),
                &serde_json::to_vec(&p.connection)?,
            )?;
            config::store_credentials(root, &p.connection, &c)?;
            crate::update::install_bootstrap(root)?;
            Ok(json!({"self_source":profile}))
        }
        NodeCommand::Operation { id } => Ok(serde_json::to_value(node.operation(&id)?)?),
        NodeCommand::ProfileAdd { file: path } => {
            let p: Profile = serde_json::from_value(file(&path)?)?;
            let c: Credentials = serde_json::from_value(input()?)?;
            node.add_profile(&p, &c)?;
            Ok(json!({"profile":p.id}))
        }
        NodeCommand::ProfileRemove { id } => {
            node.remove_profile(&id)?;
            Ok(json!({"removed":id}))
        }
        NodeCommand::Login { profile, otp } => {
            let code = if otp {
                Some(rpassword::prompt_password("One-time code: ")?)
            } else {
                None
            };
            node.login(&profile, code.as_deref())?;
            Ok(json!({"authenticated":true}))
        }
        NodeCommand::Plans { profile } => node.request(&profile, "GET", "/v1/plans", None),
        NodeCommand::Subscribe { profile, plan } => {
            node.subscribe(&profile, &plan)?;
            Ok(json!({"subscribed":true}))
        }
        NodeCommand::Register { file: path } => {
            let i: Instance = serde_json::from_value(file(&path)?)?;
            node.register(&i)?;
            Ok(json!({"registered":i.id}))
        }
        NodeCommand::Check { instance } => {
            let signed = node.offer(&instance)?;
            Ok(
                json!({"release_id":signed.identity()?,"release":signed.verify(&node.profile(&node.instance(&instance)?.profile)?.connection.release_public_key)?}),
            )
        }
        NodeCommand::Stage { instance, offline } => {
            let signed = if let Some(path) = &offline {
                serde_json::from_value(file(&path.join("release.json"))?)?
            } else {
                node.offer(&instance)?
            };
            Ok(serde_json::to_value(node.stage(
                &instance,
                signed,
                offline.as_deref(),
            )?)?)
        }
        NodeCommand::ResumeStage { operation, offline } => Ok(serde_json::to_value(
            node.resume_stage(&operation, offline.as_deref())?,
        )?),
        NodeCommand::Prepare { operation } => Ok(serde_json::to_value(
            node.transition(&operation, "prepare")?,
        )?),
        NodeCommand::Apply { operation } => {
            Ok(serde_json::to_value(node.transition(&operation, "apply")?)?)
        }
        NodeCommand::Reconcile { operation } => Ok(serde_json::to_value(
            node.transition(&operation, "reconcile")?,
        )?),
        NodeCommand::Rollback { operation } => Ok(serde_json::to_value(
            node.transition(&operation, "rollback")?,
        )?),
        NodeCommand::Verify { instance, version } => {
            let (op, pending) = node.dependency(&instance, version.as_deref())?;
            if let Some(version) = version {
                ensure!(
                    op.release.version == version,
                    "managed version differs from running product"
                );
            }
            Ok(
                json!({"verified":true,"activation_pending":pending,"instance":instance,"version":op.release.version,"release_id":op.release_id,"operation":op.id}),
            )
        }
        NodeCommand::Export {
            operation,
            destination,
        } => {
            node.export(&operation, &destination)?;
            Ok(json!({"exported":true}))
        }
        NodeCommand::Uninstall => node.schedule_uninstall(),
    }
}
