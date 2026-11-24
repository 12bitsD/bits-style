use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;
use serde::Deserialize;

use crate::model::Report;
use crate::scanner;

const FORMAT_RULES: [&str; 5] = ["BCS001", "BCS002", "BCS007", "BCS008", "BCS009"];

#[derive(Debug, Args)]
pub struct BenchArgs {
    #[arg(long, default_value = "benchmarks/manifest.toml")]
    manifest: PathBuf,
    #[arg(long, default_value = "corpus/bad")]
    corpus: PathBuf,
    #[arg(long, default_value = "target/bench-cache")]
    cache_dir: PathBuf,
    #[arg(long)]
    offline: bool,
    #[arg(long)]
    strict: bool,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    negative: Vec<NegativeRepo>,
}

#[derive(Debug, Deserialize)]
struct NegativeRepo {
    name: String,
    url: String,
    commit: String,
    #[serde(default = "default_source_path")]
    path: String,
    #[serde(default)]
    exclude: Vec<String>,
}

fn default_source_path() -> String {
    "src".to_owned()
}

#[derive(Debug, Deserialize)]
struct CorpusCase {
    dimension: String,
    expect: Vec<String>,
    before: String,
    after: String,
}

pub fn run(args: BenchArgs) -> Result<u8, String> {
    let mut output = String::new();
    let (passed, total) = eval_positive(&args.corpus, &mut output)?;
    let (violations, warnings) = eval_negative(&args, &mut output)?;
    output.push_str(&format!(
        "\nSummary: positive={passed}/{total} passed; \
         negative_violations={violations} negative_warnings={warnings}\n"
    ));
    std::io::stdout()
        .write_all(output.as_bytes())
        .map_err(|error| error.to_string())?;
    let failed = passed < total || (args.strict && violations > 0);
    Ok(u8::from(failed))
}

fn eval_positive(corpus: &Path, output: &mut String) -> Result<(usize, usize), String> {
    output.push_str("Positive corpus (before recall / after clean):\n");
    let mut cases = load_cases(corpus)?;
    cases.sort_by(|a, b| a.0.cmp(&b.0));
    let mut passed = 0;
    for (name, root, case) in &cases {
        if eval_case(name, root, case, output)? {
            passed += 1;
        }
    }
    Ok((passed, cases.len()))
}

fn eval_case(
    name: &str,
    root: &Path,
    case: &CorpusCase,
    output: &mut String,
) -> Result<bool, String> {
    let before = rule_counts(&scan_l1(&root.join(&case.before), &[])?);
    let after = rule_counts(&scan_l1(&root.join(&case.after), &[])?);
    let missing: Vec<&str> = case
        .expect
        .iter()
        .filter(|rule| !before.contains_key(*rule))
        .map(String::as_str)
        .collect();
    let remaining: Vec<&str> = case
        .expect
        .iter()
        .filter(|rule| after.contains_key(*rule))
        .map(String::as_str)
        .collect();
    let ok = missing.is_empty() && remaining.is_empty();
    output.push_str(&format!(
        "  {name} [{}]: expect={:?} before={} after={} {}\n",
        case.dimension,
        case.expect,
        format_counts(&before),
        format_counts(&after),
        if ok { "OK" } else { "FAIL" }
    ));
    if !missing.is_empty() {
        output.push_str(&format!("    missing in before: {missing:?}\n"));
    }
    if !remaining.is_empty() {
        output.push_str(&format!("    still in after: {remaining:?}\n"));
    }
    Ok(ok)
}

fn eval_negative(args: &BenchArgs, output: &mut String) -> Result<(usize, usize), String> {
    let source = std::fs::read_to_string(&args.manifest)
        .map_err(|error| format!("{}: {error}", args.manifest.display()))?;
    let manifest: Manifest =
        toml::from_str(&source).map_err(|error| format!("{}: {error}", args.manifest.display()))?;
    output.push_str("\nNegative corpus (L1 rules only; format/profile rules excluded):\n");
    let mut violations = 0;
    let mut warnings = 0;
    for repo in &manifest.negative {
        let (repo_violations, repo_warnings) =
            eval_repo(repo, &args.cache_dir, args.offline, output)?;
        violations += repo_violations;
        warnings += repo_warnings;
    }
    Ok((violations, warnings))
}

fn eval_repo(
    repo: &NegativeRepo,
    cache_dir: &Path,
    offline: bool,
    output: &mut String,
) -> Result<(usize, usize), String> {
    let checkout = prepare_repo(repo, cache_dir, offline)?;
    let report = scan_l1(&checkout.join(&repo.path), &repo.exclude)?;
    let mut by_rule: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for violation in &report.results {
        let entry = by_rule.entry(violation.rule_id.as_str()).or_default();
        if violation.severity == "violation" {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let details = by_rule
        .iter()
        .map(|(rule, (v, w))| format!("{rule}: {v}v/{w}w"))
        .collect::<Vec<_>>()
        .join(", ");
    let short_commit = &repo.commit[..8.min(repo.commit.len())];
    let summary = &report.summary;
    output.push_str(&format!(
        "  {} ({short_commit}): files={} violations={} warnings={} [{}]\n",
        repo.name, summary.files_scanned, summary.violations, summary.warnings, details
    ));
    Ok((summary.violations, summary.warnings))
}

fn load_cases(corpus: &Path) -> Result<Vec<(String, PathBuf, CorpusCase)>, String> {
    if !corpus.is_dir() {
        return Ok(Vec::new());
    }
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(corpus).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let root = entry.path();
        if !root.is_dir() {
            continue;
        }
        let manifest = root.join("case.toml");
        if !manifest.is_file() {
            continue;
        }
        let source = std::fs::read_to_string(&manifest).map_err(|error| error.to_string())?;
        let case =
            toml::from_str(&source).map_err(|error| format!("{}: {error}", manifest.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        cases.push((name, root, case));
    }
    Ok(cases)
}

fn scan_l1(path: &Path, exclude: &[String]) -> Result<Report, String> {
    let mut report = scanner::scan_path_for_bench(path, exclude)?;
    report
        .results
        .retain(|violation| !FORMAT_RULES.contains(&violation.rule_id.as_str()));
    report.finish();
    Ok(report)
}

fn rule_counts(report: &Report) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for violation in &report.results {
        *counts.entry(violation.rule_id.clone()).or_default() += 1;
    }
    counts
}

fn format_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return "{}".to_owned();
    }
    let inner = counts
        .iter()
        .map(|(rule, count)| format!("{rule}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{inner}}}")
}

fn prepare_repo(repo: &NegativeRepo, cache_dir: &Path, offline: bool) -> Result<PathBuf, String> {
    let checkout = cache_dir.join(&repo.name);
    if !checkout.join(".git").is_dir() {
        if offline {
            return Err(format!("{} is not cached (offline mode)", repo.name));
        }
        std::fs::create_dir_all(&checkout).map_err(|error| error.to_string())?;
        git(&checkout, &["init", "-q"])?;
        git(&checkout, &["remote", "add", "origin", &repo.url])?;
    }
    let current = Command::new("git")
        .current_dir(&checkout)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
