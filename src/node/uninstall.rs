//! Independent node removal. Only the identity-marked node root is removed.
use super::*;
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};
impl Node {
    pub fn remove_profile(&self, id: &str) -> Result<()> {
        let count: u32 = self.db.query_row(
            "SELECT COUNT(*) FROM instances WHERE profile=?1",
            [id],
            |r| r.get(0),
        )?;
        ensure!(count == 0, "profile still owns installed instances");
        let p = self.profile(id)?;
        let home = self.profile_home(id)?;
        let _ = self.request(id, "POST", "/v1/logout", Some(&json!({})));
        config::delete_secret(&home, &p.connection, "session")?;
        config::delete_secret(&home, &p.connection, "credentials")?;
        self.db
            .execute("DELETE FROM subscriptions WHERE profile=?1", [id])?;
        self.db.execute("DELETE FROM profiles WHERE id=?1", [id])?;
        fs::remove_dir_all(home)?;
        Ok(())
    }
    pub fn schedule_uninstall(&self) -> Result<Value> {
        let unresolved:u32=self.db.query_row("SELECT COUNT(*) FROM operations WHERE status NOT IN ('applied','rolled_back','cancelled') OR (json_extract(body,'$.online')=1 AND json_extract(body,'$.reported')=0)",[],|r|r.get(0))?;
        ensure!(
            unresolved == 0,
            "reconcile updates and outstanding receipts before uninstall"
        );
        let server_db = self.root.join("server.sqlite");
        ensure!(
            !server_db.exists(),
            "server business state requires a separate server root"
        );
        let mut st = self.db.prepare("SELECT id FROM profiles")?;
        let ids: Vec<String> = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in ids {
            let p = self.profile(&id)?;
            let home = self.profile_home(&id)?;
            let _ = self.request(&id, "POST", "/v1/logout", Some(&json!({})));
            config::delete_secret(&home, &p.connection, "session")?;
            config::delete_secret(&home, &p.connection, "credentials")?;
        }
        if self.root.join("config.json").exists() {
            let c = Config::load(&self.root)?;
            config::delete_secret(&self.root, &c, "session")?;
            config::delete_secret(&self.root, &c, "credentials")?;
        }
        let parent = self.root.parent().context("node root requires a parent")?;
        let cleanup = parent.join(format!(
            ".gesac-cleaner-{}{}",
            runtime::random_id(),
            std::env::consts::EXE_SUFFIX
        ));
        fs::copy(std::env::current_exe()?, &cleanup)?;
        config::harden_new_executable(&cleanup)?;
        Command::new(&cleanup)
            .arg("__remove-node")
            .arg(&self.root)
            .arg(&self.device)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(
            json!({"scheduled":true,"device":self.device,"root":self.root,"managed_products":"retained"}),
        )
    }
}
pub fn clean(root: &Path, device: &str) -> Result<()> {
    runtime::name(device)?;
    let original = root.to_path_buf();
    let root = root.canonicalize()?;
    ensure!(root == original, "cleanup root must be canonical");
    ensure!(
        root.parent().is_some() && root.components().count() >= 3,
        "invalid cleanup root"
    );
    ensure!(
        !root.join("server.sqlite").exists(),
        "server business database is outside node removal"
    );
    let marker: Value =
        serde_json::from_slice(&config::read_private_file(&root.join("node.json"), 4096)?)?;
    ensure!(marker["device"] == device, "node identity changed");
    let deadline = Instant::now() + Duration::from_secs(30);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("node.lock"))?;
    while lock.try_lock_exclusive().is_err() {
        ensure!(Instant::now() < deadline, "node still active");
        thread::sleep(Duration::from_millis(100));
    }
    let updater = if root.join("update.lock").exists() {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("update.lock"))?;
        f.try_lock_exclusive().context("self update is active")?;
        Some(f)
    } else {
        None
    };
    drop(updater);
    drop(lock);
    // Renaming withdraws the complete installation before recursive removal.
    let tomb = root.with_file_name(format!(".gesac-removed-{device}"));
    ensure!(!tomb.exists(), "removal already needs reconciliation");
    fs::rename(&root, &tomb)?;
    fs::remove_dir_all(&tomb)?;
    let exe = std::env::current_exe()?;
    #[cfg(unix)]
    fs::remove_file(exe)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let path = exe.to_string_lossy().replace('\'', "''");
        let script = format!("Start-Sleep -Seconds 2; Remove-Item -LiteralPath '{path}' -Force");
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
            .creation_flags(0x08000000)
            .spawn()?;
    }
    Ok(())
}
