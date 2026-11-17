use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::language::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    Changed,
    Staged,
}

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub absolute: PathBuf,
    pub relative: String,
    pub language: Language,
}

pub fn discover(
    inputs: &[PathBuf],
    scope: Scope,
    no_ignore: bool,
    excludes: &[String],
) -> Result<(PathBuf, Vec<DiscoveredFile>), String> {
    let root = discovery_root(inputs)?;
    let mut paths = BTreeSet::new();
    match scope {
        Scope::All => {
            for input in inputs {
                collect_input(input, no_ignore, &mut paths)?;
            }
        }
        Scope::Changed => git_changed(&root, false, &mut paths)?,
        Scope::Staged => git_changed(&root, true, &mut paths)?,
    }

    let mut files = Vec::new();
    for absolute in paths {
        let Some(language) = Language::from_path(&absolute) else {
            continue;
        };
        let relative = absolute
            .strip_prefix(&root)
            .unwrap_or(&absolute)
            .to_string_lossy()
            .replace('\\', "/");
        files.push(DiscoveredFile {
            absolute,
            relative,
            language,
        });
    }
    let mut globs = GlobSetBuilder::new();
    for pattern in excludes
        .iter()
        .map(String::as_str)
        .chain(["**/*.generated.*", "**/*.gen.*"])
    {
        globs.add(Glob::new(pattern).map_err(|error| error.to_string())?);
    }
    let globs = globs.build().map_err(|error| error.to_string())?;
    files.retain(|file| !globs.is_match(&file.relative));
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok((root, files))
}

fn discovery_root(inputs: &[PathBuf]) -> Result<PathBuf, String> {
    let first = inputs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let absolute = if first.is_absolute() {
        first
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(first)
    };
    let metadata = std::fs::metadata(&absolute).map_err(|error| error.to_string())?;
    let root = if metadata.is_dir() {
        absolute
    } else {
        absolute.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    root.canonicalize().map_err(|error| error.to_string())
}

fn collect_input(
    input: &Path,
    no_ignore: bool,
    output: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let metadata =
        std::fs::metadata(input).map_err(|error| format!("{}: {error}", input.display()))?;
    if metadata.is_file() {
        if let Ok(path) = input.canonicalize() {
            output.insert(path);
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }

    let mut builder = WalkBuilder::new(input);
    builder
        .hidden(false)
        .git_ignore(!no_ignore)
        .git_global(!no_ignore)
        .git_exclude(!no_ignore)
        .follow_links(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git" | "node_modules" | "target" | "dist" | "build" | ".venv" | "venv"
            )
        });
    for entry in builder.build() {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            output.insert(
                entry
                    .path()
                    .canonicalize()
                    .map_err(|error| error.to_string())?,
            );
        }
    }
    Ok(())
}

fn git_changed(root: &Path, staged: bool, output: &mut BTreeSet<PathBuf>) -> Result<(), String> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["diff", "--name-only", "--diff-filter=ACMR", "-z"]);
    if staged {
        command.arg("--cached");
    } else {
        command.arg("HEAD");
    }
    let result = command
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }
    for name in String::from_utf8_lossy(&result.stdout)
        .split('\0')
        .filter(|name| !name.is_empty())
    {
        let path = root.join(name);
        if path.is_file() {
            output.insert(path.canonicalize().map_err(|error| error.to_string())?);
        }
    }
    Ok(())
}
