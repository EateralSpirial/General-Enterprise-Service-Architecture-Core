use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub file: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    600
}
impl Adapter {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.file.is_absolute()
                && self.file.is_file()
                && (1..=3600).contains(&self.timeout_seconds)
                && self.args.len() <= 64,
            "invalid local adapter"
        );
        Ok(())
    }
    pub fn call(&self, input: &Value) -> Result<Value> {
        self.validate()?;
        let mut child = Command::new(&self.file)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("adapter unavailable")?;
        let mut stdin = child.stdin.take().context("adapter input unavailable")?;
        let body = serde_json::to_vec(input)?;
        thread::spawn(move || {
            let _ = stdin.write_all(&body);
        });
        let stdout = child.stdout.take().context("adapter output unavailable")?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.take(65537).read_to_end(&mut bytes).map(|_| bytes);
            let _ = tx.send(result);
        });
        let deadline = Instant::now() + Duration::from_secs(self.timeout_seconds);
        let mut output = None;
        let mut status = None;
        loop {
            if output.is_none() {
                if let Ok(result) = rx.try_recv() {
                    output = Some(result?);
                }
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let (Some(status), Some(bytes)) = (status, output.as_ref()) {
                ensure!(
                    status.success() && bytes.len() <= 65536,
                    "adapter outcome requires reconciliation"
                );
                return Ok(serde_json::from_slice(bytes)?);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("adapter timed out; reconcile original operation");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Node {
    pub fn transition(&self, id: &str, action: &str) -> Result<Operation> {
        let mut op = self.operation(id)?;
        let instance = self.instance(&op.instance)?;
        self.verify_operation_files(&op)?;
        ensure!(
            ["prepare", "apply", "reconcile", "rollback"].contains(&action),
            "invalid lifecycle action"
        );
        if action == "reconcile"
            && ["applied", "rolled_back", "cancelled"].contains(&op.status.as_str())
        {
            self.report(&mut op)?;
            return Ok(op);
        }
        match action {
            "prepare" => ensure!(
                op.status == "staged" || op.status == "prepared",
                "prepare requires staged operation"
            ),
            "apply" => ensure!(op.status == "prepared", "apply requires prepared operation"),
            "rollback" => ensure!(
                op.status == "applied" || op.status == "unknown",
                "rollback requires applied or uncertain operation"
            ),
            _ => ensure!(
                ["unknown", "preparing", "applying", "rolling_back"].contains(&op.status.as_str()),
                "operation does not require reconciliation"
            ),
        }
        if action == "rollback" {
            let active: Option<String> = self.db.query_row(
                "SELECT active FROM instances WHERE id=?1",
                [&op.instance],
                |r| r.get(0),
            )?;
            ensure!(
                active.is_none() || active.as_deref() == Some(&op.id),
                "a newer release owns this instance"
            );
        }
        op.status = match action {
            "prepare" => "preparing",
            "apply" => "applying",
            "rollback" => "rolling_back",
            _ => "unknown",
        }
        .into();
        self.save(&op)?;
        let result = if let Some(adapter) = &instance.adapter {
            adapter.call(&json!({"schema":1,"action":action,"operation":op.id,"instance":op.instance,"release_id":op.release_id,"release":op.release,"directory":op.directory}))
        } else {
            Ok(
                json!({"status":match action{"prepare"=>"prepared","apply"=>"applied","rollback"=>"rolled_back",_=>"unknown"}}),
            )
        };
        match result {
            Ok(value) => {
                let next = value["status"].as_str().context("adapter status missing")?;
                ensure!(
                    ["prepared", "applied", "rolled_back", "unknown"].contains(&next),
                    "adapter returned unsupported status"
                );
                op.status = next.into();
                op.result = value;
            }
            Err(_) => {
                op.status = "unknown".into();
                op.result =
                    json!({"error":"adapter outcome unknown; reconcile the original operation"});
            }
        }
        self.save(&op)?;
        if op.status == "applied" {
            self.db.execute(
                "UPDATE instances SET active=?1 WHERE id=?2",
                params![op.id, op.instance],
            )?;
        }
        if op.status == "rolled_back" {
            self.db.execute(
                "UPDATE instances SET active=(SELECT id FROM operations WHERE instance=?1 AND id<>?2 AND status='applied' ORDER BY rowid DESC LIMIT 1) WHERE id=?1 AND active=?2",
                params![op.instance, op.id],
            )?;
        }
        self.report(&mut op)?;
        Ok(op)
    }
}
