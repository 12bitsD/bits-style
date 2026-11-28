use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn git_root(input: &Path) -> Result<PathBuf, String> {
    let directory = if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(Path::new("."))
    };
    let output = Command::new("git")
        .current_dir(directory)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    PathBuf::from(root)
        .canonicalize()
        .map_err(|e| e.to_string())
}

pub(crate) fn git_changed(
    root: &Path,
    staged: bool,
    output: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut command = Command::new("git");
    command.current_dir(root).args([
        "diff",
        "--name-only",
        "--diff-filter=ACMR",
        "-z",
    ]);
    command.arg(if staged { "--cached" } else { "HEAD" });
    let result = command
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    for name in String::from_utf8_lossy(&result.stdout)
        .split('\0')
        .filter(|n| !n.is_empty())
    {
        let path = root.join(name);
        if path.is_file() {
            output.insert(path.canonicalize().map_err(|e| e.to_string())?);
        }
    }
    Ok(())
}

pub(crate) fn git_untracked(
    root: &Path,
    specs: &[String],
    no_ignore: bool,
) -> Result<Vec<String>, String> {
    let stdout = run_git_ls(root, no_ignore, false, specs)?;
    Ok(stdout
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}

pub(crate) fn run_git_ls(
    root: &Path,
    no_ignore: bool,
    include_cached: bool,
    specs: &[String],
) -> Result<String, String> {
    let mut command = Command::new("git");
    command.current_dir(root).arg("ls-files");
    if include_cached {
        command.arg("--cached");
    }
    command.args(["--others", "-z"]);
    if !no_ignore {
        command.arg("--exclude-standard");
    }
    if !specs.is_empty() {
        command.arg("--").args(specs);
    }
    let output = command
        .output()
        .map_err(|e| format!("failed to run git ls-files: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
