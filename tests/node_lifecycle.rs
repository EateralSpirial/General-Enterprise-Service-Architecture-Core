use gesac::{
    config::{Config, Credentials},
    node::{Node, Profile},
    runtime,
};
use std::{
    fs,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[test]
fn self_uninstall_removes_encrypted_profile_and_preserves_external_data() {
    let parent = std::env::temp_dir().join(format!("gesac-removal-{}", runtime::random_id()));
    gesac::config::ensure_private_dir(&parent).unwrap();
    let root = parent.join("node");
    let sentinel = parent.join("managed-product.txt");
    fs::write(&sentinel, "preserved").unwrap();
    {
        let node = Node::open(&root).unwrap();
        node.add_profile(
            &Profile {
                id: "host".into(),
                host_id: "host-one".into(),
                connection: Config {
                    schema: 1,
                    center_url: "https://127.0.0.1:1".into(),
                    account: "fixture".into(),
                    release_public_key: "11".repeat(32),
                    channel: "stable".into(),
                    allow_http_loopback: false,
                    credential_store: "sealed".into(),
                    ca_certificate: None,
                },
            },
            &Credentials {
                password: "synthetic-password".into(),
                passphrase: "synthetic-passphrase".into(),
            },
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gesac"))
        .arg("uninstall")
        .env("GESAC_HOME", &root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    while root.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    assert!(!root.exists(), "node configuration was not removed");
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "preserved");
    // Windows' final cleanup process exits after deleting its own copied executable.
    for _ in 0..50 {
        if fs::read_dir(&parent).unwrap().count() == 1 {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    fs::remove_dir_all(parent).unwrap();
}
