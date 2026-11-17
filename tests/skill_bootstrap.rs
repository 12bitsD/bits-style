use std::fs;
use std::process::Command;

#[test]
fn bootstrap_is_non_destructive_and_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let agents = root.join("AGENTS.md");
    fs::write(&agents, "existing project instructions\n").unwrap();

    let script = format!(
        "{}/skills/bits-style/scripts/bootstrap.sh",
        env!("CARGO_MANIFEST_DIR")
    );
    let path = format!(
        "{}/target/release:{}",
        env!("CARGO_MANIFEST_DIR"),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(&script)
        .arg(root)
        .arg("HEAD~2")
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&agents).unwrap(),
        "existing project instructions\n"
    );
    assert!(root.join("bits.toml").exists());
    assert!(root.join(".github/workflows/bits.yml").exists());
    assert!(root.join("bits-baseline.json").exists());

    let output = Command::new(&script)
        .arg(root)
        .env(
            "PATH",
            format!(
                "{}/target/release:{}",
                env!("CARGO_MANIFEST_DIR"),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&agents).unwrap(),
        "existing project instructions\n"
    );
}
