mod admin;
mod client;
mod model;
mod server;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gesac", version, about = "Enterprise software delivery")]
struct Cli {
    #[arg(long, env = "GESAC_ENTERPRISE_HOME", global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Configure {
        #[arg(long)]
        host: String,
    },
    Login {
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        credentials_stdin: bool,
    },
    Logout,
    Software,
    Select {
        product: String,
    },
    Install {
        product: String,
    },
    Reconcile {
        product: String,
        #[arg(long, value_parser = ["installed", "failed"])] // GATE
        outcome: String,
        #[arg(long)]
        reason: String,
    },
    AutoUpdate {
        product: String,
        #[arg(value_parser = ["enable", "disable"])] // GATE: supported task actions
        action: String,
    },
    Sync,
    Status,
    Run {
        product: String,
    },
    Server {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "127.0.0.1:4185")]
        listen: String,
        #[arg(long, default_value = "https://yanhan.chat/gesa/api/")]
        public_url: String,
    },
    Admin {
        #[arg(long)]
        root: PathBuf,
        #[command(subcommand)]
        command: admin::Command,
    },
}

fn main() // GATE
{
    if let Err(error) = run()
    // GATE: unified actionable failure boundary
    {
        eprintln!(
            "{}",
            serde_json::json!({"error": error.to_string(), "recovery": "Correct the reported condition, then repeat the command. For interrupted installations inspect gesac status before retrying."})
        ); // GATE
        std::process::exit(1); // GATE
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Server {
            root,
            listen,
            public_url,
        } => server::serve(root, listen, public_url),
        Command::Admin { root, command } => admin::run(&root, command),
        command => client::run(cli.home, command),
    }
}
