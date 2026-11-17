use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn bootstrap_is_non_destructive_and_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let agents = root.join("AGENTS.md");
    fs::write(&agents, "existing project instructions\n").unwrap();

    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills/bits-style/scripts/bootstrap.sh")
        .to_string_lossy()
        .replace('\\', "/");
    let bits = Path::new(env!("CARGO_BIN_EXE_bits"));
    let mut paths = vec![bits.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(paths).unwrap();
    let run = |base_revision: Option<&str>| {
        let mut command = Command::new("bash");
        command.arg(&script).arg(root).env("PATH", &path);
        if let Some(base_revision) = base_revision {
            command.arg(base_revision);
        }
        command.output().unwrap()
    };

    let output = run(Some("HEAD~2"));
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

    let output = run(None);
    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(&agents).unwrap(),
        "existing project instructions\n"
    );
}
