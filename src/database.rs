//! Each role owns an independent private SQLite database.
use crate::config;
use anyhow::{ensure, Result};
use rusqlite::Connection;
use std::{path::Path, time::Duration};

pub fn open(root: &Path, filename: &str) -> Result<Connection> {
    config::ensure_private_dir(root)?;
    let path = root.join(filename);
    if !path.exists() {
        config::write_private_file(&path, &[])?;
    }
    let metadata = std::fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "database must be a regular private file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "database must be private"
        );
    }
    let connection = Connection::open(&path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
    )?;
    let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(version <= 1, "database version requires newer GESAC");
    Ok(connection)
}
