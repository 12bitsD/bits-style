mod repos;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;

use crate::model::Report;
use crate::scanner;

use repos::{CorpusCase, Manifest, NegativeRepo, load_cases, prepare_repo};

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

#[derive(Default)]
struct RepoStats {
    violations: usize,
    warnings: usize,
    functions: usize,
    warned_functions: usize,
    cognitive: Vec<usize>,
    complexity_pairs: Vec<(usize, usize)>,
}

impl RepoStats {
    fn absorb(&mut self, other: Self) {
        self.violations += other.violations;
        self.warnings += other.warnings;
        self.functions += other.functions;
        self.warned_functions += other.warned_functions;
        self.cognitive.extend(other.cognitive);
        self.complexity_pairs.extend(other.complexity_pairs);
    }
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
    Ok(u8::from(passed < total || (args.strict && violations > 0)))
}

fn eval_positive(
    corpus: &Path,
    output: &mut String,
) -> Result<(usize, usize), String> {
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

fn eval_negative(
    args: &BenchArgs,
    output: &mut String,
) -> Result<(usize, usize), String> {
    let source = std::fs::read_to_string(&args.manifest)
        .map_err(|error| format!("{}: {error}", args.manifest.display()))?;
    let manifest: Manifest = toml::from_str(&source)
        .map_err(|error| format!("{}: {error}", args.manifest.display()))?;
    output.push_str(
        "\nNegative corpus (L1 rules only; format/profile rules excluded):\n",
    );
    let mut total = RepoStats::default();
    for repo in &manifest.negative {
        total.absorb(eval_repo(repo, &args.cache_dir, args.offline, output)?);
    }
    render_cognitive_distribution(&total, output);
    Ok((total.violations, total.warnings))
}

fn eval_repo(
    repo: &NegativeRepo,
    cache_dir: &Path,
    offline: bool,
    output: &mut String,
) -> Result<RepoStats, String> {
    let checkout = prepare_repo(repo, cache_dir, offline)?;
    let scan = scanner::scan_path_for_bench(
        &checkout.join(&repo.path),
        &repo.exclude,
    )?;
    let details = format_rule_breakdown(&scan.report);
    let short_commit = &repo.commit[..8.min(repo.commit.len())];
    let s = &scan.report.summary;
    let warn_pct = percent(s.functions_with_warnings, s.functions_checked);
    output.push_str(&format!(
        "  {} ({short_commit}): files={} fns={} violations={} warnings={} warned_fns={:.2}% [{details}]\n",
        repo.name,
        s.files_scanned,
        s.functions_checked,
        s.violations,
        s.warnings,
        warn_pct
    ));
    Ok(RepoStats {
        violations: s.violations,
        warnings: s.warnings,
        functions: s.functions_checked,
        warned_functions: s.functions_with_warnings,
        cognitive: scan.cognitive_scores,
        complexity_pairs: scan.complexity_pairs,
    })
}

fn format_rule_breakdown(report: &Report) -> String {
    let mut by_rule: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for violation in &report.results {
        let entry = by_rule.entry(violation.rule_id.as_str()).or_default();
        if violation.severity == "violation" {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    by_rule
        .iter()
        .map(|(rule, (v, w))| format!("{rule}: {v}v/{w}w"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64) * 100.0 / (whole as f64)
    }
}

fn percentile(sorted: &[usize], pct: f64) -> usize {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((sorted.len() - 1) as f64 * pct).round() as usize;
    sorted[rank]
}

fn render_cognitive_distribution(stats: &RepoStats, output: &mut String) {
    let mut scores = stats.cognitive.clone();
    scores.sort_unstable();
    let over = |limit| scores.iter().filter(|&&score| score > limit).count();
    let high_cc = stats
        .complexity_pairs
        .iter()
        .filter(|(cc, _)| *cc > 10)
        .count();
    let flat = stats
        .complexity_pairs
        .iter()
        .filter(|(cc, cog)| *cc > 10 && *cog <= 10)
        .count();
    let tangled = stats
        .complexity_pairs
        .iter()
        .filter(|(cc, cog)| *cc > 10 && *cog > 15)
        .count();
    output.push_str(&format!(
        "\nCognitive complexity (cognitive_v1) across {} functions:\n  P50={} P90={} P95={} P99={} max={}\n  >10: {:.2}%  >15: {:.2}%  >20: {:.2}%  >25: {:.2}%\n  L1 warned functions overall: {:.2}% ({}/{})\n  Cyclomatic >10: {high_cc} functions; cognitive <=10 (flat dispatch): {flat} ({:.1}%); cognitive >15 (tangled): {tangled}\n",
        scores.len(),
        percentile(&scores, 0.50),
        percentile(&scores, 0.90),
        percentile(&scores, 0.95),
        percentile(&scores, 0.99),
        scores.last().copied().unwrap_or(0),
        percent(over(10), scores.len()),
        percent(over(15), scores.len()),
        percent(over(20), scores.len()),
        percent(over(25), scores.len()),
        percent(stats.warned_functions, stats.functions),
        stats.warned_functions,
        stats.functions,
        percent(flat, high_cc),
    ));
}

fn scan_l1(path: &Path, exclude: &[String]) -> Result<Report, String> {
    scanner::scan_path_for_bench(path, exclude).map(|scan| scan.report)
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
