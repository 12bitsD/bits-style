mod diff;
mod git;
mod paths;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::language::Language;

use paths::{
    absolute_path, collect_input, discovery_root, excluded_directory,
    exclusion_globs, is_unsupported_source, pathspecs,
};

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
    pub explicit_unsupported: Vec<String>,
    pub dependency_context: Vec<DiscoveredFile>,
}

struct FinalizeInput<'a> {
    root: PathBuf,
    paths: BTreeSet<PathBuf>,
    excludes: &'a [String],
    line_scopes: Option<&'a BTreeMap<String, LineScope>>,
    explicit_files: &'a BTreeSet<PathBuf>,
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
    let mut found = BTreeSet::new();
    let mut explicit_files = BTreeSet::new();
    match scope {
        Scope::All => {
            for input in inputs {
                collect_input(
                    input,
                    no_ignore,
                    &mut found,
                    &mut explicit_files,
                )?;
            }
        }
        Scope::Changed => git::git_changed(&root, false, &mut found)?,
        Scope::Staged => git::git_changed(&root, true, &mut found)?,
        Scope::Diff(_) => unreachable!(),
    }
    finalize_files(FinalizeInput {
        root,
        paths: found,
        excludes,
        line_scopes: None,
        explicit_files: &explicit_files,
    })
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
    let root = git::git_root(&absolute_path(&first)?)?;
    let specs = pathspecs(inputs, &root)?;
    let explicit_files = collect_explicit_files(inputs);
    let mut line_scopes = diff::git_diff_lines(&root, base, &specs)?;
    let mut found = collect_diff_paths(&root, &line_scopes)?;
    for relative in git::git_untracked(&root, &specs, no_ignore)? {
        let path = root.join(&relative);
        if path.is_file() {
            found.insert(path.canonicalize().map_err(|e| e.to_string())?);
            line_scopes.insert(relative, LineScope::WholeFile);
        }
    }
    let mut discovery = finalize_files(FinalizeInput {
        root: root.clone(),
        paths: found,
        excludes,
        line_scopes: Some(&line_scopes),
        explicit_files: &explicit_files,
    })?;
    discovery.dependency_context =
        git_dependency_context(&root, no_ignore, excludes, &discovery.files)?;
    Ok(discovery)
}

fn collect_explicit_files(inputs: &[PathBuf]) -> BTreeSet<PathBuf> {
    inputs
        .iter()
        .filter(|path| std::fs::metadata(path).is_ok_and(|m| m.is_file()))
        .filter_map(|path| path.canonicalize().ok())
        .collect()
}

fn collect_diff_paths(
    root: &Path,
    scopes: &BTreeMap<String, LineScope>,
) -> Result<BTreeSet<PathBuf>, String> {
    let mut found = BTreeSet::new();
    for (relative, scope) in scopes {
        if matches!(scope, LineScope::Added(lines) if lines.is_empty()) {
            continue;
        }
        let path = root.join(relative);
        if path.is_file() {
            found.insert(path.canonicalize().map_err(|e| e.to_string())?);
        }
    }
    Ok(found)
}

fn git_dependency_context(
    root: &Path,
    no_ignore: bool,
    excludes: &[String],
    changed: &[DiscoveredFile],
) -> Result<Vec<DiscoveredFile>, String> {
    let output = git::run_git_ls(root, no_ignore, true, &[])?;
    let changed: BTreeSet<&str> =
        changed.iter().map(|f| f.relative.as_str()).collect();
    let globs = exclusion_globs(excludes)?;
    let mut files = Vec::new();
    for relative in output.split('\0').filter(|n| !n.is_empty()) {
        if changed.contains(relative)
            || globs.is_match(Path::new(relative))
            || excluded_directory(relative)
        {
            continue;
        }
        if let Some(entry) = context_entry(root, relative) {
            files.push(entry);
        }
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

fn context_entry(root: &Path, relative: &str) -> Option<DiscoveredFile> {
    let absolute = root.join(relative);
    let metadata = std::fs::symlink_metadata(&absolute).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    let language = Language::from_path(&absolute)?;
    Some(DiscoveredFile {
        absolute,
        relative: relative.to_owned(),
        language,
        line_scope: LineScope::WholeFile,
    })
}

fn finalize_files(input: FinalizeInput<'_>) -> Result<Discovery, String> {
    let mut files = Vec::new();
    let mut unsupported = Vec::new();
    let mut explicit_unsupported = Vec::new();
    for absolute in input.paths {
        let relative = absolute
            .strip_prefix(&input.root)
            .unwrap_or(&absolute)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(language) = Language::from_path(&absolute) {
            let line_scope = input
                .line_scopes
                .and_then(|scopes| scopes.get(&relative))
                .cloned()
                .unwrap_or(LineScope::WholeFile);
            files.push(DiscoveredFile {
                absolute,
                line_scope,
                relative,
                language,
            });
        } else if is_unsupported_source(&absolute) {
            if input.explicit_files.contains(&absolute) {
                explicit_unsupported.push(relative.clone());
            }
            unsupported.push(relative);
        }
    }
    let globs = exclusion_globs(input.excludes)?;
    let keep = |rel: &str| !globs.is_match(rel) && !excluded_directory(rel);
    files.retain(|file| keep(&file.relative));
    unsupported.retain(|file| keep(file));
    explicit_unsupported.retain(|file| keep(file));
    unsupported.sort();
    explicit_unsupported.sort();
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(Discovery {
        root: input.root,
        files,
        unsupported,
        explicit_unsupported,
        dependency_context: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::LineScope;
    use super::diff::{decode_git_path, parse_diff_lines};

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
