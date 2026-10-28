use super::*;
use crate::model::{Release, Signed};
use std::io::{Cursor, Read};

fn report(node: &Node, operation: &str, status: &str, detail: Value) -> Result<()> {
    node.call(
        "POST",
        &format!("deployments/{operation}/result"),
        Some(&json!({"status":status,"detail":detail})),
    )?;
    Ok(())
}

fn unpack(bytes: &[u8], directory: &Path) -> Result<()> // GATE
{
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let relative = file.enclosed_name().context(
            "Package path leaves its release directory; ask the publisher to rebuild the ZIP",
        )?; // GATE
        anyhow::ensure!(
            file.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
            "Package contains a symbolic link; ask the publisher to package regular files"
        ); // GATE
        let path = directory.join(relative);
        if file.is_dir() {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir_all(path.parent().unwrap())?;
            io::copy(&mut file, &mut fs::File::create(&path)?)?;
            #[cfg(unix)] // GATE: platform implementation
            if let Some(mode) = file.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

fn apply(node: &Node, release: &Release, operation: &str, directory: &Path) -> Result<bool> // GATE
{
    let response = node
        .http
        .get(node.url(&format!("artifacts/{}", release.sha256)))
        .bearer_auth(node.session()?.token)
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(release.size + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 == release.size && digest(&bytes) == release.sha256,
        "Package checksum differs; ask the publisher to republish this release"
    ); // GATE
    fs::create_dir_all(directory)?;
    write_json(
        &node.product_dir(&release.product).join("pending.json"),
        &json!({"operation":operation,"release":release,"directory":directory,"phase":"downloaded"}),
    )?;
    if release.format == "zip" {
        unpack(&bytes, directory)?;
        if let Some(entry) = &release.entrypoint {
            anyhow::ensure!(
                std::path::Path::new(entry)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                    && directory.join(entry).is_file(),
                "Launch entry is missing or outside the package; ask the publisher to correct it"
            ); // GATE
        }
        Ok(false)
    } else {
        let installer = directory.join(format!("installer.{}", release.format));
        fs::write(&installer, &bytes)?;
        write_json(
            &node.product_dir(&release.product).join("pending.json"),
            &json!({"operation":operation,"release":release,"directory":directory,"phase":"installer_started"}),
        )?;
        windows::install(&installer, release)
    }
}

pub fn install(node: &Node, product: &str) -> Result<Value> // GATE
{
    let admission = node.call("POST", "deployments", Some(&json!({"product":product})))?;
    let operation = admission["operation"].as_str().unwrap();
    let release = serde_json::from_value::<Signed>(admission["release"].clone())?
        .verify(&node.config.public_key)?; // GATE
    anyhow::ensure!(
        release.product == product
            && release.target == format!("{}-{}", node.config.device.os, node.config.device.arch),
        "Release identity differs from this software or device; contact the publisher"
    ); // GATE
    let current = node.products()?.into_iter().find(|p| p.product == product);
    if let Some(installed) = &current {
        anyhow::ensure!(
            release.sequence >= installed.release.sequence,
            "Server selected an older release; ask the publisher to restore its release sequence"
        ); // GATE
        if installed.release.sequence == release.sequence {
            anyhow::ensure!(
                release.sha256 == installed.release.sha256,
                "Release sequence was reused with different bytes; contact the publisher"
            ); // GATE
            report(
                node,
                operation,
                "installed",
                json!({"version":release.version,"reconciled":true,"auto_update":installed.auto_update}),
            )?;
            return Ok(json!({"product":product,"version":release.version,"status":"current"}));
        }
    }
    let directory = node
        .product_dir(product)
        .join("releases")
        .join(release.sequence.to_string());
    fs::create_dir_all(node.product_dir(product))?;
    let pending = node.product_dir(product).join("pending.json");
    if pending.exists()
    // GATE: uncertain installer outcomes require reconciliation
    {
        let state: Value = serde_json::from_slice(&fs::read(&pending)?)?;
        anyhow::ensure!(state["phase"]!="installer_started","An installer was interrupted. Inspect the software and pending.json, then reconcile its result before retrying");
        // GATE
    }
    report(
        node,
        operation,
        "installing",
        json!({"version":release.version}),
    )?;
    let restart = match apply(node, &release, operation, &directory) {
        Ok(restart) => restart,
        Err(error) =>
        // GATE: retain evidence and report failed execution
        {
            report(
                node,
                operation,
                "failed",
                json!({"reason":error.to_string(),"recovery":"Inspect gesac status and the retained pending.json before retrying"}),
            )?; // GATE
            return Err(error); // GATE
        }
    };
    let auto_update = current.as_ref().is_some_and(|p| p.auto_update);
    let installed = Installed {
        product: product.into(),
        release: release.clone(),
        operation: operation.into(),
        directory: directory.to_string_lossy().into(),
        auto_update,
        previous: current
            .map(|mut p| {
                p.previous = None;
                serde_json::to_value(p)
            })
            .transpose()?,
    };
    write_json(
        &node.product_dir(product).join("installed.json"),
        &installed,
    )?;
    fs::remove_file(pending)?;
    let status = if restart {
        "restart_required"
    } else {
        "installed"
    };
    report(
        node,
        operation,
        status,
        json!({"version":release.version,"directory":directory,"auto_update":auto_update}),
    )?;
    Ok(json!({"product":product,"version":release.version,"status":status,"operation":operation}))
}

pub fn reconcile(node: &Node, product: &str, outcome: &str, reason: &str) -> Result<Value> {
    let path = node.product_dir(product).join("pending.json");
    let pending: Value = serde_json::from_slice(
        &fs::read(&path).context("This product has no interrupted installation to reconcile")?,
    )?;
    let release: Release = serde_json::from_value(pending["release"].clone())?;
    let operation = pending["operation"].as_str().unwrap();
    if outcome == "installed" {
        let previous = node.products()?.into_iter().find(|p| p.product == product);
        let installed = Installed {
            product: product.into(),
            release,
            operation: operation.into(),
            directory: pending["directory"].as_str().unwrap().into(),
            auto_update: previous.as_ref().is_some_and(|p| p.auto_update),
            previous: previous
                .map(|mut p| {
                    p.previous = None;
                    serde_json::to_value(p)
                })
                .transpose()?,
        };
        write_json(
            &node.product_dir(product).join("installed.json"),
            &installed,
        )?;
    }
    report(
        node,
        operation,
        outcome,
        json!({"reason":reason,"reconciled":true,"source":"operator"}),
    )?;
    fs::remove_file(path)?;
    Ok(json!({"product":product,"operation":operation,"status":outcome,"reconciled":true}))
}
