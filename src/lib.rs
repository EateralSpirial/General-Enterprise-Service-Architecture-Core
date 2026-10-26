pub mod auth;
pub mod commands;
pub mod config;
pub mod database;
#[cfg(target_os = "macos")]
mod keychain;
pub mod node;
pub mod protocol;
pub mod runtime;
mod sealed;
pub mod server;
pub mod update;

pub const BUILD_VERSION: &str = match option_env!("GESAC_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
