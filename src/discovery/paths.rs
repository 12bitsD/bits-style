use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;

pub(crate) fn is_unsupported_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
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
    )
}

pub(crate) fn exclusion_globs(
    excludes: &[String],
) -> Result<globset::GlobSet, String> {
    let mut globs = GlobSetBuilder::new();
    for pattern in excludes
        .iter()
        .map(String::as_str)
        .chain(["**/*.generated.*", "**/*.gen.*"])
    {
        globs.add(Glob::new(pattern).map_err(|e| e.to_string())?);
    }
    globs.build().map_err(|e| e.to_string())
}

pub(crate) fn excluded_directory(relative: &str) -> bool {
    relative.split('/').any(|component| {
        matches!(
            component,
            ".git"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | ".venv"
                | "venv"
        )
    })
}

pub(crate) fn discovery_root(inputs: &[PathBuf]) -> Result<PathBuf, String> {
    let first = inputs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let absolute = absolute_path(&first)?;
    let metadata = std::fs::metadata(&absolute).map_err(|e| e.to_string())?;
    let root = if metadata.is_dir() {
        absolute
    } else {
        absolute.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    root.canonicalize().map_err(|e| e.to_string())
}

pub(crate) fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    Ok(absolute.canonicalize().unwrap_or(absolute))
}

pub(crate) fn pathspecs(
    inputs: &[PathBuf],
    root: &Path,
) -> Result<Vec<String>, String> {
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
        let spec = if relative.as_os_str().is_empty() {
            ".".to_owned()
        } else {
            relative.to_string_lossy().replace('\\', "/")
        };
        output.push(spec);
    }
    Ok(output)
}

pub(crate) fn collect_input(
    input: &Path,
    no_ignore: bool,
    output: &mut BTreeSet<PathBuf>,
    explicit_files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let metadata = std::fs::metadata(input)
        .map_err(|e| format!("{}: {e}", input.display()))?;
    if metadata.is_file() {
        if let Ok(path) = input.canonicalize() {
            explicit_files.insert(path.clone());
            output.insert(path);
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    walk_directory(input, no_ignore, output)
}

fn walk_directory(
    input: &Path,
    no_ignore: bool,
    output: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let mut builder = WalkBuilder::new(input);
    builder
        .hidden(false)
        .git_ignore(!no_ignore)
        .git_global(!no_ignore)
        .git_exclude(!no_ignore)
        .follow_links(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !excluded_directory(name.as_ref())
        });
    for entry in builder.build() {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            output.insert(
                entry.path().canonicalize().map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(())
}
