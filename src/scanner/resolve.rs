use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use crate::discovery::DiscoveredFile;
use crate::language::Language;

pub(super) fn python_package_context(
    file: &DiscoveredFile,
) -> Option<(String, String)> {
    if file.language != Language::Python {
        return None;
    }
    let mut abs_dir = file.absolute.parent()?;
    let mut rel_dir =
        Path::new(&file.relative).parent().unwrap_or(Path::new(""));
    let mut pkg_name = None;
    let mut pkg_dir = rel_dir;
    while abs_dir.join("__init__.py").is_file()
        || abs_dir.join("__init__.pyi").is_file()
    {
        pkg_name = abs_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        pkg_dir = rel_dir;
        rel_dir = rel_dir.parent().unwrap_or(Path::new(""));
        abs_dir = abs_dir.parent()?;
    }
    Some((pkg_name?, pkg_dir.to_string_lossy().replace('\\', "/")))
}

pub(super) fn resolve_dependency<'a>(
    current: &str,
    specifier: &str,
    python_package: Option<&(String, String)>,
    known: &BTreeSet<&'a str>,
) -> Option<&'a str> {
    if let Some(absolute) = specifier.strip_prefix('/') {
        return resolve_absolute_dep(absolute, python_package, known);
    }
    let parent = Path::new(current).parent().unwrap_or(Path::new(""));
    let base = normalize_path(&parent.join(specifier))
        .to_string_lossy()
        .replace('\\', "/");
    resolve_module_base(&base, known)
}

fn resolve_absolute_dep<'a>(
    absolute: &str,
    python_package: Option<&(String, String)>,
    known: &BTreeSet<&'a str>,
) -> Option<&'a str> {
    let path = Path::new(absolute);
    let first = path
        .components()
        .next()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some((pkg_name, pkg_dir)) = python_package {
        if first == *pkg_name {
            let rest = path.strip_prefix(&first).unwrap_or(path);
            let base = normalize_path(&Path::new(pkg_dir).join(rest))
                .to_string_lossy()
                .replace('\\', "/");
            return resolve_module_base(&base, known);
        }
        if pkg_dir.is_empty() {
            return None;
        }
    }
    let base = normalize_path(path).to_string_lossy().replace('\\', "/");
    resolve_module_base(&base, known)
}

fn resolve_module_base<'a>(
    base: &str,
    known: &BTreeSet<&'a str>,
) -> Option<&'a str> {
    let candidates = if Path::new(base).extension().is_some() {
        vec![base.to_owned()]
    } else {
        vec![
            format!("{base}.ts"),
            format!("{base}.tsx"),
            format!("{base}.js"),
            format!("{base}.jsx"),
            format!("{base}.py"),
            format!("{base}.rs"),
            format!("{base}/mod.rs"),
            format!("{base}/index.ts"),
            format!("{base}/index.tsx"),
            format!("{base}/index.js"),
            format!("{base}/index.jsx"),
            format!("{base}/__init__.py"),
        ]
    };
    candidates
        .into_iter()
        .find_map(|candidate| known.get(candidate.as_str()).copied())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                output.pop();
            }
            other => output.push(other.as_os_str()),
        }
    }
    output
}
