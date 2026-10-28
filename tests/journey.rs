#[test]
fn company_deployment_journey() {
    let status = std::process::Command::new(if cfg!(windows) { "python" } else { "python3" })
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/journey.py"))
        .arg(env!("CARGO_BIN_EXE_gesac"))
        .status()
        .unwrap();
    assert!(status.success()); // GATE: integration acceptance
}
