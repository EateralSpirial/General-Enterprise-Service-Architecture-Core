use super::*;
use crate::model::Release;

#[cfg(not(windows))] // GATE: platform implementation
pub fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> // GATE
{
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, bytes)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?; // GATE
    Ok(())
}

#[cfg(not(windows))] // GATE: platform implementation
pub fn read_secret(path: &Path) -> Result<Vec<u8>> // GATE
{
    Ok(fs::read(path)?)
}

#[cfg(windows)] // GATE: native current-user session protection
pub fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> // GATE
{
    fs::write(
        path,
        windows_dpapi::encrypt_data(bytes, windows_dpapi::Scope::User, None)?,
    )?; // GATE
    Ok(())
}

#[cfg(windows)] // GATE: native current-user session protection
pub fn read_secret(path: &Path) -> Result<Vec<u8>> // GATE
{
    Ok(windows_dpapi::decrypt_data(
        &fs::read(path)?,
        windows_dpapi::Scope::User,
        None,
    )?) // GATE
}

pub fn install(path: &Path, release: &Release) -> Result<bool> // GATE
{
    let mut command = if release.format == "msi" {
        let mut command = Process::new("msiexec.exe");
        command
            .arg("/i")
            .arg(path)
            .args(["/quiet", "/norestart", "/L*v"])
            .arg(path.with_extension("log"));
        command
    } else {
        Process::new(path)
    };
    let status = command.args(&release.arguments).status()?;
    let code = status.code().unwrap_or(-1);
    anyhow::ensure!([0,3010].contains(&code),"Installer returned {code}; inspect its log. For machine-wide software run from an administrator PowerShell"); // GATE
    Ok(code == 3010)
}

pub fn task(node: &Node, enable: bool, scope: &str) -> Result<()> // GATE
{
    let name = format!(
        "GESAC-Enterprise-{}",
        &digest(node.home.to_string_lossy().as_bytes())[..12]
    );
    let mut command = Process::new("schtasks.exe");
    if enable {
        let bin = node.home.join("bin");
        fs::create_dir_all(&bin)?;
        let executable = bin.join("gesac.exe");
        let current = std::env::current_exe()?;
        if current != executable {
            fs::copy(current, &executable)?;
        }
        let action = format!(
            "\"{}\" --home \"{}\" sync",
            executable.display(),
            node.home.display()
        );
        let script = node.home.join("sync.vbs");
        let script_text = format!(
            "Set shell = CreateObject(\"WScript.Shell\")\r\nWScript.Quit shell.Run(\"{}\", 0, True)\r\n",
            action.replace('"', "\"\"")
        );
        let mut script_bytes = vec![0xff, 0xfe];
        script_bytes.extend(script_text.encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&script, script_bytes)?;
        let action = format!("wscript.exe //B //Nologo \"{}\"", script.display());
        command.args([
            "/Create", "/F", "/SC", "MINUTE", "/MO", "15", "/IT", "/TN", &name, "/TR", &action,
        ]);
        if scope == "machine"
            || node
                .products()?
                .iter()
                .any(|p| p.auto_update && p.release.scope == "machine")
        // GATE: machine-wide updates need an elevated task
        {
            command.args(["/RL", "HIGHEST"]); // GATE
        }
    } else {
        let remaining = node.products()?.iter().filter(|p| p.auto_update).count();
        if remaining == 0 {
            return Ok(());
        }
        if remaining > 1 {
            return Ok(());
        }
        command.args(["/Delete", "/F", "/TN", &name]);
    }
    let output = command.output()?;
    anyhow::ensure!(output.status.success(),"Windows task configuration failed: {}. For machine-wide updates run this command from an administrator PowerShell",String::from_utf8_lossy(&output.stderr)); // GATE
    Ok(())
}
