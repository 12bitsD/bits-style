use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::discovery::{self, Scope};
use crate::layers::{LayerPolicy, LayerRuleConfig};
use crate::model::{Report, Violation};
use crate::taint::IoPolicy;

use super::BaselineArgs;
use super::pipeline::{ScanEnv, scan_report};

#[derive(Debug, Default, Deserialize)]
pub(super) struct Config {
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub io_sources: Vec<String>,
    #[serde(default)]
    pub layers: Vec<LayerRuleConfig>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Baseline {
    pub results: Vec<Violation>,
}

pub(super) fn load_config(path: Option<&Path>) -> Result<Config, String> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_str(&source)
        .map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn check_min_version(config: &Config) -> Result<(), String> {
    let Some(required) = config.min_version.as_deref() else {
        return Ok(());
    };
    let required_semver = parse_semver(required).ok_or_else(|| {
        format!(
            "invalid min_version '{required}' in config \
             (expected MAJOR.MINOR.PATCH)"
        )
    })?;
    let current = env!("CARGO_PKG_VERSION");
    let current_semver = parse_semver(current).unwrap_or((0, 0, 0));
    if current_semver < required_semver {
        return Err(format!(
            "bits version {current} is older than required min_version \
             {required}; upgrade with: cargo install --git \
             https://github.com/12bitsD/bits-style --tag v{required} --locked"
        ));
    }
    Ok(())
}

fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().strip_prefix('v').unwrap_or(raw.trim());
    let core = trimmed.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub(super) fn load_baseline(path: &Path) -> Result<Baseline, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&source)
        .map_err(|error| format!("{}: {error}", path.display()))
}

pub(super) fn apply_baseline(report: &mut Report, baseline: &Baseline) {
    let files: BTreeSet<&str> = baseline
        .results
        .iter()
        .map(|result| result.file.as_str())
        .collect();
    let mut fingerprints = count_fingerprints(&baseline.results);
    let before = report.results.len();
    report.results.retain(|result| {
        if !files.contains(result.file.as_str()) {
            return true;
        }
        !consume_fingerprint(&mut fingerprints, result)
    });
    report.summary.suppressed_violations = before - report.results.len();
    report.summary.stale_baseline_entries = fingerprints.values().sum();
}

pub(super) fn execute_baseline_prune(
    args: &BaselineArgs,
) -> Result<u8, String> {
    if args.jobs == 0 {
        return Err("--jobs must be greater than zero".to_owned());
    }
    let config = load_config(args.config.as_deref())?;
    check_min_version(&config)?;
    let inputs = resolve_inputs(&args.paths);
    let discovery = discovery::discover(
        &inputs,
        Scope::All,
        args.no_ignore,
        &config.exclude,
    )?;
    let policy = IoPolicy::new(&config.io_sources)?;
    let layers = LayerPolicy::compile(&config.layers)?;
    let env = ScanEnv {
        jobs: args.jobs,
        include_passed: false,
        policy: &policy,
        layers: &layers,
        diff_base: None,
    };
    let current = scan_report(&discovery, &env);
    if !current.scan_errors.is_empty() {
        return Err(format!(
            "cannot prune baseline while {} scan error(s) remain",
            current.scan_errors.len()
        ));
    }
    prune_baseline_file(&args.prune, &current)
}

fn resolve_inputs(paths: &[PathBuf]) -> Vec<PathBuf> {
    if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.to_vec()
    }
}

fn prune_baseline_file(path: &Path, current: &Report) -> Result<u8, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut existing: Report = serde_json::from_str(&source)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let mut active = count_fingerprints(&current.results);
    let before = existing.results.len();
    existing
        .results
        .retain(|entry| consume_fingerprint(&mut active, entry));
    let removed = before - existing.results.len();
    existing.finish();
    let rendered = serde_json::to_string_pretty(&existing)
        .map(|output| output + "\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(path, rendered)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    write_prune_summary(path, removed, existing.results.len())?;
    Ok(0)
}

fn write_prune_summary(
    path: &Path,
    removed: usize,
    remaining: usize,
) -> Result<(), String> {
    let suffix = if removed == 1 { "y" } else { "ies" };
    let message = format!(
        "Pruned {removed} stale baseline entr{suffix} \
         ({remaining} remaining) -> {}\n",
        path.display()
    );
    std::io::stdout()
        .write_all(message.as_bytes())
        .map_err(|error| error.to_string())
}

type Fingerprint = (String, String, String, String);

fn baseline_fingerprint(result: &Violation) -> Fingerprint {
    (
        result.rule_id.clone(),
        result.file.clone(),
        result.message.clone(),
        result.actual.to_string(),
    )
}

fn count_fingerprints(results: &[Violation]) -> BTreeMap<Fingerprint, usize> {
    let mut counts = BTreeMap::new();
    for result in results {
        *counts.entry(baseline_fingerprint(result)).or_default() += 1;
    }
    counts
}

fn consume_fingerprint(
    counts: &mut BTreeMap<Fingerprint, usize>,
    entry: &Violation,
) -> bool {
    let key = baseline_fingerprint(entry);
    match counts.get_mut(&key) {
        Some(count) if *count > 0 => {
            *count -= 1;
            true
        }
        _ => false,
    }
}
