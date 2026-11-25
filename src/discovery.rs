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

pub struct Discovery {
    pub root: PathBuf,
    pub files: Vec<DiscoveredFile>,
    pub unsupported: Vec<String>,
    pub dependency_context: Vec<DiscoveredFile>,
}

pub fn discover(
    inputs: &[PathBuf],
    scope: Scope,
    no_ignore: bool,
    excludes: &[String],
) -> Result<Discovery, String> {
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
) -> Result<Discovery, String> {
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
    let mut discovery = finalize_files(root.clone(), paths, excludes, Some(&line_scopes))?;
    discovery.dependency_context =
        git_dependency_context(&root, no_ignore, excludes, &discovery.files)?;
    Ok(discovery)
}

fn git_dependency_context(
    root: &Path,
    no_ignore: bool,
    excludes: &[String],
    changed: &[DiscoveredFile],
) -> Result<Vec<DiscoveredFile>, String> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["ls-files", "--cached", "--others", "-z"]);
    if !no_ignore {
        command.arg("--exclude-standard");
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to list dependency context: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    let changed: BTreeSet<&str> = changed.iter().map(|file| file.relative.as_str()).collect();
    let globs = exclusion_globs(excludes)?;
    let mut files = Vec::new();
    for relative in String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|name| !name.is_empty())
    {
        if changed.contains(relative) || globs.is_match(Path::new(relative)) {
            continue;
        }
        let relative = relative.to_owned();
        if excluded_directory(&relative) {
            continue;
        }
        let absolute = root.join(&relative);
        let Ok(metadata) = std::fs::symlink_metadata(&absolute) else {
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let Some(language) = Language::from_path(&absolute) else {
            continue;
        };
        files.push(DiscoveredFile {
            absolute,
            relative,
            language,
            line_scope: LineScope::WholeFile,
        });
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

fn finalize_files(
    root: PathBuf,
    paths: BTreeSet<PathBuf>,
    excludes: &[String],
    line_scopes: Option<&BTreeMap<String, LineScope>>,
) -> Result<Discovery, String> {
    let mut files = Vec::new();
    let mut unsupported = Vec::new();
    for absolute in paths {
        let relative = absolute
            .strip_prefix(&root)
            .unwrap_or(&absolute)
            .to_string_lossy()
            .replace('\\', "/");
        let Some(language) = Language::from_path(&absolute) else {
            if matches!(
                absolute.extension().and_then(|value| value.to_str()),
                Some(
                    "java"
                        | "kt"
                        | "swift"
                        | "rb"
                        | "php"
                        | "cs"
                        | "scala"
                        | "lua"
                        | "zig"
                        | "ex"
                        | "exs"
                        | "hs"
                        | "ml"
                        | "sh"
                        | "bash"
                        | "zsh"
                )
            ) {
                unsupported.push(relative);
            }
            continue;
        };
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
    let globs = exclusion_globs(excludes)?;
    files.retain(|file| !globs.is_match(&file.relative) && !excluded_directory(&file.relative));
    unsupported.retain(|file| !globs.is_match(file) && !excluded_directory(file));
    unsupported.sort();
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(Discovery {
        root,
        files,
        unsupported,
        dependency_context: Vec::new(),
    })
}

fn exclusion_globs(excludes: &[String]) -> Result<globset::GlobSet, String> {
    let mut globs = GlobSetBuilder::new();
    for pattern in excludes
        .iter()
        .map(String::as_str)
        .chain(["**/*.generated.*", "**/*.gen.*"])
    {
        globs.add(Glob::new(pattern).map_err(|error| error.to_string())?);
    }
    globs.build().map_err(|error| error.to_string())
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
    let revision = Command::new("git")
        .current_dir(root)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ])
        .output()
        .map_err(|error| format!("failed to validate diff commit: {error}"))?;
    if !revision.status.success() {
        return Err(format!("invalid diff commit: {base}"));
    }
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--end-of-options",
            base,
            "--",
        ])
        .args(pathspecs)
        .output()
        .map_err(|error| format!("failed to run git diff: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }

    parse_diff_lines(&String::from_utf8_lossy(&output.stdout))
}

fn parse_diff_lines(text: &str) -> Result<BTreeMap<String, LineScope>, String> {
    let mut files: BTreeMap<String, LineScope> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.trim_end_matches(['\t', '\r']);
            current = if path == "/dev/null" {
                None
            } else {
                let decoded = decode_git_path(path);
                let path = decoded.strip_prefix("b/").unwrap_or(&decoded).to_owned();
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
        let (old_count, new_start, new_count) =
            parse_hunk(line).ok_or_else(|| format!("invalid diff hunk: {line}"))?;
        if new_count > 0 && current.is_none() {
            return Err(format!("diff hunk has no destination file: {line}"));
        }
        let mut old_remaining = old_count;
        let mut new_remaining = new_count;
        let mut new_line = new_start;
        while old_remaining > 0 || new_remaining > 0 {
            let body = lines.next().ok_or_else(|| {
                format!(
                    "truncated hunk for {}",
                    current.as_deref().unwrap_or("/dev/null")
                )
            })?;
            match body.as_bytes().first() {
                Some(b' ') => {
                    if old_remaining == 0 || new_remaining == 0 {
                        return Err(format!("diff hunk line counts do not match: {line}"));
                    }
                    old_remaining -= 1;
                    new_remaining -= 1;
                    new_line += 1;
                }
                Some(b'+') => {
                    if new_remaining == 0 {
                        return Err(format!("diff hunk has too many added lines: {line}"));
                    }
                    if let Some(path) = current.as_ref()
                        && let Some(LineScope::Added(lines)) = files.get_mut(path)
                    {
                        lines.insert(new_line);
                    }
                    new_remaining -= 1;
                    new_line += 1;
                }
                Some(b'-') => {
                    if old_remaining == 0 {
                        return Err(format!("diff hunk has too many deleted lines: {line}"));
                    }
                    old_remaining -= 1;
                }
                Some(b'\\') => {}
                _ => return Err(format!("invalid diff hunk body: {body}")),
            }
        }
    }
    Ok(files)
}

fn decode_git_path(path: &str) -> String {
    if !(path.starts_with('"') && path.ends_with('"') && path.len() >= 2) {
        return path.to_owned();
    }
    let bytes = &path.as_bytes()[1..path.len() - 1];
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        if index >= bytes.len() {
            break;
        }
        match bytes[index] {
            b'n' => output.push(b'\n'),
            b't' => output.push(b'\t'),
            b'r' => output.push(b'\r'),
            b'\\' => output.push(b'\\'),
            b'"' => output.push(b'"'),
            digit @ b'0'..=b'7' => {
                let mut value = digit - b'0';
                for _ in 0..2 {
                    if index + 1 < bytes.len() && matches!(bytes[index + 1], b'0'..=b'7') {
                        index += 1;
                        value = value * 8 + (bytes[index] - b'0');
                    }
                }
                output.push(value);
            }
            other => output.push(other),
        }
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn parse_hunk(line: &str) -> Option<(usize, usize, usize)> {
    let mut parts = line.strip_prefix("@@ ")?.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    if parts.next()? != "@@" {
        return None;
    }
    let (_, old_count) = parse_hunk_range(old)?;
    let (new_start, new_count) = parse_hunk_range(new)?;
    Some((old_count, new_start, new_count))
}

fn parse_hunk_range(range: &str) -> Option<(usize, usize)> {
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

#[cfg(test)]
mod tests {
    use super::{LineScope, decode_git_path, parse_diff_lines};

    #[test]
    fn parses_added_lines_next_to_no_newline_markers() {
        let patch = "diff --git a/sample.py b/sample.py\n--- a/sample.py\n+++ b/sample.py\n@@ -1 +1,2 @@\n value = 0\n+global_counter = 0\n\\ No newline at end of file\n";
        let files = parse_diff_lines(patch).unwrap();
        let Some(LineScope::Added(lines)) = files.get("sample.py") else {
            panic!("missing sample.py diff scope");
        };
        assert_eq!(lines.iter().copied().collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn rejects_truncated_hunks() {
        let patch = "diff --git a/sample.py b/sample.py\n--- a/sample.py\n+++ b/sample.py\n@@ -0,0 +1,2 @@\n+first\n";
        let error = parse_diff_lines(patch).unwrap_err();
        assert!(error.contains("truncated hunk"), "{error}");
    }

    #[test]
    fn decodes_quoted_git_paths() {
        assert_eq!(decode_git_path("\"b/a\\\"b.py\""), "b/a\"b.py");
        assert_eq!(decode_git_path("\"b/a\\nb.py\""), "b/a\nb.py");
        assert_eq!(decode_git_path("b/plain.py"), "b/plain.py");
    }
}
