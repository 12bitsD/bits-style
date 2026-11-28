use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Manifest {
    #[serde(default)]
    pub negative: Vec<NegativeRepo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NegativeRepo {
    pub name: String,
    pub url: String,
    pub commit: String,
    #[serde(default = "default_source_path")]
    pub path: String,
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_source_path() -> String {
    "src".to_owned()
}

#[derive(Debug, Deserialize)]
pub(crate) struct CorpusCase {
    pub dimension: String,
    pub expect: Vec<String>,
    pub before: String,
    pub after: String,
}

pub(crate) fn load_cases(
    corpus: &Path,
) -> Result<Vec<(String, PathBuf, CorpusCase)>, String> {
    if !corpus.is_dir() {
        return Ok(Vec::new());
    }
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(corpus).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let root = entry.path();
        let manifest = root.join("case.toml");
        if !root.is_dir() || !manifest.is_file() {
            continue;
        }
        let source =
            std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
        let case = toml::from_str(&source)
            .map_err(|e| format!("{}: {e}", manifest.display()))?;
        cases.push((
            entry.file_name().to_string_lossy().into_owned(),
            root,
            case,
        ));
    }
    Ok(cases)
}

pub(crate) fn prepare_repo(
    repo: &NegativeRepo,
    cache_dir: &Path,
    offline: bool,
) -> Result<PathBuf, String> {
    let checkout = cache_dir.join(&repo.name);
    if !checkout.join(".git").is_dir() {
        if offline {
            return Err(format!("{} is not cached (offline mode)", repo.name));
        }
        std::fs::create_dir_all(&checkout).map_err(|e| e.to_string())?;
        git(&checkout, &["init", "-q"])?;
        git(&checkout, &["remote", "add", "origin", &repo.url])?;
    }
    let current = Command::new("git")
        .current_dir(&checkout)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    if current != repo.commit {
        if offline {
            return Err(format!(
                "{} is not at pinned commit (offline mode)",
                repo.name
            ));
        }
        git(
            &checkout,
            &["fetch", "-q", "--depth", "1", "origin", &repo.commit],
        )?;
        git(&checkout, &["checkout", "-q", "--detach", "FETCH_HEAD"])?;
    }
    Ok(checkout)
}

fn git(root: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}
