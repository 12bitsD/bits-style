use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::language::Language;

#[derive(Debug, Clone)]
pub enum Scope {
    All,
    Changed,
    Staged,
    Diff(String),
}

#[derive(Debug, Clone, Default)]
pub enum LineScope {
    #[default]
    WholeFile,
    Added(BTreeSet<usize>),
}

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub absolute: PathBuf,
    pub relative: String,
    pub language: Language,
    pub line_scope: LineScope,
}

pub fn discover(
    inputs: &[PathBuf],
    scope: Scope,
    no_ignore: bool,
    excludes: &[String],
) -> Result<(PathBuf, Vec<DiscoveredFile>), String> {
    if let Scope::Diff(base) = &scope {
        return discover_diff(inputs, base, no_ignore, excludes);
    }

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
        Scope::Diff(_) => unreachable!(),
    }
    finalize_files(root, paths, excludes, None)
}

fn discover_diff(
    inputs: &[PathBuf],
    base: &str,
    no_ignore: bool,
    excludes: &[String],
) -> Result<(PathBuf, Vec<DiscoveredFile>), String> {
    let first = inputs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let first = absolute_path(&first)?;
    let root = git_root(&first)?;
    let pathspecs = pathspecs(inputs, &root)?;
    let mut line_scopes = git_diff_lines(&root, base, &pathspecs)?;
    let mut paths = BTreeSet::new();

    for (relative, scope) in &line_scopes {
        if matches!(scope, LineScope::Added(lines) if lines.is_empty()) {
            continue;
        }
        let path = root.join(relative);
        if path.is_file() {
            paths.insert(path.canonicalize().map_err(|error| error.to_string())?);
        }
    }
    for relative in git_untracked(&root, &pathspecs, no_ignore)? {
        let path = root.join(&relative);
        if path.is_file() {
            paths.insert(path.canonicalize().map_err(|error| error.to_string())?);
            line_scopes.insert(relative, LineScope::WholeFile);
        }
    }
    finalize_files(root, paths, excludes, Some(&line_scopes))
}

fn finalize_files(
    root: PathBuf,
    paths: BTreeSet<PathBuf>,
    excludes: &[String],
    line_scopes: Option<&BTreeMap<String, LineScope>>,
) -> Result<(PathBuf, Vec<DiscoveredFile>), String> {
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
            line_scope: line_scopes
                .and_then(|scopes| scopes.get(&relative))
                .cloned()
                .unwrap_or(LineScope::WholeFile),
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
    files.retain(|file| !globs.is_match(&file.relative) && !excluded_directory(&file.relative));
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok((root, files))
}

fn excluded_directory(relative: &str) -> bool {
    relative.split('/').any(|component| {
        matches!(
            component,
            ".git" | "node_modules" | "target" | "dist" | "build" | ".venv" | "venv"
        )
    })
}

fn discovery_root(inputs: &[PathBuf]) -> Result<PathBuf, String> {
    let first = inputs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let absolute = absolute_path(&first)?;
    let metadata = std::fs::metadata(&absolute).map_err(|error| error.to_string())?;
    let root = if metadata.is_dir() {
        absolute
    } else {
        absolute.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    root.canonicalize().map_err(|error| error.to_string())
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    Ok(absolute.canonicalize().unwrap_or(absolute))
}

fn git_root(input: &Path) -> Result<PathBuf, String> {
    let directory = if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(Path::new("."))
    };
    let output = Command::new("git")
        .current_dir(directory)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    PathBuf::from(root)
        .canonicalize()
        .map_err(|error| error.to_string())
}

fn pathspecs(inputs: &[PathBuf], root: &Path) -> Result<Vec<String>, String> {
    let mut output = Vec::new();
    for input in inputs {
        let absolute = absolute_path(input)?;
        let relative = absolute.strip_prefix(root).map_err(|_| {
            format!(
                "diff input {} is outside repository {}",
                absolute.display(),
                root.display()
            )
        })?;
        if relative.as_os_str().is_empty() {
            output.push(".".to_owned());
        } else {
            output.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(output)
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

fn git_diff_lines(
    root: &Path,
    base: &str,
    pathspecs: &[String],
) -> Result<BTreeMap<String, LineScope>, String> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            base,
            "--",
        ])
        .args(pathspecs)
        .output()
        .map_err(|error| format!("failed to run git diff: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut files: BTreeMap<String, LineScope> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            current = if path == "/dev/null" {
                None
            } else {
                let path = path.strip_prefix("b/").unwrap_or(path).to_owned();
                files
                    .entry(path.clone())
                    .or_insert_with(|| LineScope::Added(BTreeSet::new()));
                Some(path)
            };
            continue;
        }
        if !line.starts_with("@@") {
            continue;
        }
        let Some(path) = current.as_ref() else {
            continue;
        };
        let Some((start, count)) = parse_hunk(line) else {
            continue;
        };
        let Some(LineScope::Added(lines)) = files.get_mut(path) else {
            continue;
        };
        lines.extend(start..start + count);
    }
    Ok(files)
}

fn parse_hunk(line: &str) -> Option<(usize, usize)> {
    let range = line.split_whitespace().find(|part| part.starts_with('+'))?;
    let range = range.trim_start_matches('+');
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    Some((start.parse().ok()?, count.parse().ok()?))
}

fn git_untracked(
    root: &Path,
    pathspecs: &[String],
    no_ignore: bool,
) -> Result<Vec<String>, String> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["ls-files", "--others", "-z"]);
    if !no_ignore {
        command.arg("--exclude-standard");
    }
    command.arg("--").args(pathspecs);
    let output = command
        .output()
        .map_err(|error| format!("failed to run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}
