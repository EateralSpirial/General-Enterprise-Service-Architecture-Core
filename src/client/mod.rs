mod install;
mod windows;

use crate::{
    model::{digest, id, write_json, Config, Device, Installed, Login, Session},
    Command,
};
use anyhow::{Context, Result};
use fs2::FileExt;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command as Process,
};

pub struct Node {
    pub home: PathBuf,
    pub config: Config,
    pub http: Client,
}

fn home(argument: Option<PathBuf>) -> PathBuf {
    argument.unwrap_or_else(|| {
        #[cfg(windows)] // GATE: platform implementation
        let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("GESAC");
        #[cfg(not(windows))] // GATE: platform implementation
        let base = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/share/gesac");
        base.join("enterprise")
    })
}

fn http() -> Result<Client> // GATE
{
    Ok(Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("gesac-enterprise/", env!("CARGO_PKG_VERSION")))
        .build()?) // GATE: preserve origin during requests
}

impl Node {
    pub fn open(home: PathBuf) -> Result<Self> {
        let config = serde_json::from_slice(&fs::read(home.join("config.json")).context(
            "Configure the service first: gesac configure --host https://yanhan.chat/gesa/api/",
        )?)?;
        Ok(Self {
            home,
            config,
            http: http()?,
        })
    }

    pub fn url(&self, route: &str) -> String {
        format!("{}/{}", self.config.host, route.trim_start_matches('/'))
    }

    pub fn session(&self) -> Result<Session> // GATE
    {
        Ok(serde_json::from_slice(
            &windows::read_secret(&self.home.join("session.bin"))
                .context("Run gesac login to authenticate this device")?,
        )?)
    }

    pub fn call(&self, method: &str, route: &str, input: Option<&Value>) -> Result<Value> // GATE
    {
        let mut request = self
            .http
            .request(method.parse()?, self.url(route))
            .bearer_auth(self.session()?.token);
        if let Some(value) = input {
            request = request.json(value);
        }
        let response = request.send()?;
        let status = response.status();
        let value: Value = response.json()?;
        anyhow::ensure!(
            status.is_success(),
            "{}: {}",
            value["error"],
            value["recovery"]
        ); // GATE
        Ok(value)
    }

    pub fn register(&self) -> Result<()> {
        let result = self.call(
            "POST",
            "devices/register",
            Some(&serde_json::to_value(&self.config.device)?),
        )?;
        let mut session = self.session()?;
        session.expires_at = result["expires_at"].as_i64().unwrap_or(session.expires_at);
        windows::write_secret(
            &self.home.join("session.bin"),
            &serde_json::to_vec(&session)?,
        )?;
        Ok(())
    }

    pub fn product_dir(&self, product: &str) -> PathBuf {
        self.home.join("products").join(digest(product.as_bytes()))
    }

    pub fn installed(&self, product: &str) -> Result<Installed> {
        Ok(serde_json::from_slice(&fs::read(
            self.product_dir(product).join("installed.json"),
        )?)?)
    }

    pub fn products(&self) -> Result<Vec<Installed>> {
        let mut result = Vec::new();
        for entry in fs::read_dir(self.home.join("products"))? {
            let path = entry?.path().join("installed.json");
            if path.exists() {
                result.push(serde_json::from_slice(&fs::read(path)?)?);
            }
        }
        Ok(result)
    }
}

fn configure(root: PathBuf, host: String) -> Result<Value> // GATE
{
    let mut url = reqwest::Url::parse(&host)?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"));
    anyhow::ensure!(
        (url.scheme() == "https" || (url.scheme() == "http" && loopback))
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Use an HTTPS host address, such as https://yanhan.chat/gesa/api/"
    ); // GATE
    url.set_path(&format!("{}/", url.path().trim_end_matches('/')));
    let health: Value = http()?
        .get(url.join("health")?)
        .send()?
        .error_for_status()?
        .json()?;
    let key = health["release_public_key"]
        .as_str()
        .context("The address is not a GESAC enterprise service; check its /health endpoint")?;
    fs::create_dir_all(root.join("products"))?;
    #[cfg(unix)] // GATE: platform implementation
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?; // GATE
    }
    let path = root.join("config.json");
    let device = if path.exists() {
        serde_json::from_slice::<Config>(&fs::read(&path)?)?.device
    } else {
        Device {
            id: id(),
            name: std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "Computer".into()),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        }
    };
    let config = Config {
        host: url.as_str().trim_end_matches('/').into(),
        public_key: key.into(),
        device,
    };
    write_json(&path, &config)?;
    if root.join("session.bin").exists() {
        fs::remove_file(root.join("session.bin"))?;
    }
    Ok(
        json!({"configured":true,"host":config.host,"device":config.device,"release_public_key":key,"next":"gesac login"}),
    )
}

fn login(node: &Node, account: Option<String>, stdin: bool) -> Result<Value> // GATE
{
    let input: Login = if stdin {
        serde_json::from_reader(io::stdin())?
    } else {
        let account = match account {
            Some(value) => value,
            None => {
                print!("Company account: ");
                io::stdout().flush()?;
                let mut value = String::new();
                io::stdin().read_line(&mut value)?;
                value.trim().to_owned()
            }
        };
        Login {
            account,
            password: rpassword::prompt_password("Password: ")?,
            device: None,
        }
    };
    let input = Login {
        device: Some(node.config.device.clone()),
        ..input
    };
    let response = node
        .http
        .post(node.url("session/login"))
        .json(&input)
        .send()?;
    let status = response.status();
    let value: Value = response.json()?;
    anyhow::ensure!(
        status.is_success(),
        "{}: {}",
        value["error"],
        value["recovery"]
    ); // GATE
    let session: Session = serde_json::from_value(value)?;
    windows::write_secret(
        &node.home.join("session.bin"),
        &serde_json::to_vec(&session)?,
    )?;
    node.register()?;
    Ok(
        json!({"authenticated":true,"account":session.account,"device":node.config.device,"expires_at":session.expires_at,"next":"gesac software"}),
    )
}

fn sync(node: &Node) -> Result<Value> // GATE
{
    node.register()?;
    let mut results = Vec::new();
    for installed in node.products()? {
        if installed.auto_update
        // GATE: upgrade only opted-in installed products
        {
            let result = install::install(node, &installed.product);
            results.push(match result {
                Ok(value) => value,
                Err(error) => json!({"product":installed.product,"status":"failed","reason":error.to_string(),"recovery":"Run gesac install for this product interactively to inspect the reported condition"}), // GATE
            });
        }
    }
    let value = json!({"checked_at":crate::model::now(),"products":results});
    write_json(&node.home.join("last-sync.json"), &value)?;
    anyhow::ensure!(
        value["products"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["status"] != "failed"),
        "One or more updates failed; inspect gesac status for each product's reason and recovery"
    ); // GATE
    Ok(value)
}

pub fn run(root: Option<PathBuf>, command: Command) -> Result<()> // GATE
{
    let root = home(root);
    let result = if let Command::Configure { host } = command {
        configure(root, host)?
    } else {
        let node = Node::open(root)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(node.home.join("operation.lock"))?;
        lock.try_lock_exclusive()
            .context("Another GESAC command is running; wait for it to finish")?; // GATE
        match command {
            Command::Login {
                account,
                credentials_stdin,
            } => login(&node, account, credentials_stdin)?,
            Command::Logout => {
                node.call("DELETE", "session", None)?;
                fs::remove_file(node.home.join("session.bin"))?;
                json!({"logged_out":true,"recovery":"Run gesac login before installing or updating software"})
            }
            Command::Software => {
                node.register()?;
                node.call("GET", "catalog", None)?
            }
            Command::Select { product } => node.call(
                "POST",
                "selections",
                Some(&json!({"product":product,"device":node.config.device.id})),
            )?,
            Command::Install { product } => {
                node.register()?;
                install::install(&node, &product)?
            }
            Command::AutoUpdate { product, action } => {
                let desired = action == "enable";
                let mut installed = node
                    .installed(&product)
                    .context("Install this software before enabling automatic updates")?;
                let enabled = action == "enable";
                if installed.auto_update != desired {
                    windows::task(&node, enabled, &installed.release.scope)?;
                }
                installed.auto_update = enabled;
                write_json(
                    &node.product_dir(&product).join("installed.json"),
                    &installed,
                )?;
                node.call("POST",&format!("deployments/{}/result",installed.operation),Some(&json!({"status":"installed","detail":{"version":installed.release.version,"auto_update":enabled}})))?;
                json!({"product":product,"auto_update":enabled})
            }
            Command::Reconcile {
                product,
                outcome,
                reason,
            } => install::reconcile(&node, &product, &outcome, &reason)?,
            Command::Sync => sync(&node)?,
            Command::Status => {
                let last_sync = node.home.join("last-sync.json");
                let last_sync: Value = if last_sync.exists() {
                    serde_json::from_slice(&fs::read(last_sync)?)?
                } else {
                    Value::Null
                };
                json!({"host":node.config.host,"device":node.config.device,"products":node.products()?,"last_sync":last_sync})
            }
            Command::Run { product } => {
                let installed = node.installed(&product)?;
                let entry = installed.release.entrypoint.context(
                    "This release has no launch entry; start it using its normal Windows shortcut",
                )?;
                let child = Process::new(Path::new(&installed.directory).join(entry))
                    .current_dir(installed.directory)
                    .spawn()?;
                json!({"started":product,"pid":child.id()})
            }
            _ => unreachable!(),
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
