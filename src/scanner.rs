use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::analyze::{self, FunctionShape, ImportRecord, SyntaxAnalysis};
use crate::discovery::{self, DiscoveredFile, LineScope, Scope};
use crate::language::Language;
use crate::model::{FileFix, PassedCheck, RULE_IDS, Report, ScanError, Violation};
use crate::taint::{self, FunctionFact, IoPolicy};
use crate::{report, text};

#[derive(Debug, Parser)]
#[command(name = "bits", version, about = "Bits’s Coding Style scanner")]
pub struct Cli {
    #[command(subcommand)]
    command: TopCommand,
}

impl Cli {
    pub fn parse_args() -> Self {
        <Self as Parser>::parse()
    }
}

#[derive(Debug, Subcommand)]
enum TopCommand {
    Style(StyleArgs),
    Bench(crate::bench::BenchArgs),
}

#[derive(Debug, Args)]
struct StyleArgs {
    #[command(subcommand)]
    command: StyleCommand,
}

#[derive(Debug, Subcommand)]
enum StyleCommand {
    Fix(ScanArgs),
    Check(ScanArgs),
    Report(ScanArgs),
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Jsonl,
    Sarif,
}

#[derive(Clone, Debug, Args)]
struct ScanArgs {
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(long, conflicts_with_all = ["changed", "staged", "diff"])]
    all: bool,
    #[arg(long, conflicts_with_all = ["all", "staged", "diff"])]
    changed: bool,
    #[arg(long, conflicts_with_all = ["all", "changed", "diff"])]
    staged: bool,
    #[arg(long, conflicts_with_all = ["all", "changed", "staged"])]
    diff: bool,
    #[arg(long, requires = "diff", value_name = "COMMIT")]
    commit: Option<String>,
    #[arg(long, value_enum)]
    format: Option<OutputFormat>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    no_ignore: bool,
    #[arg(long)]
    include_passed: bool,
    #[arg(long)]
    baseline: Option<PathBuf>,
}

fn default_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |parallelism| parallelism.get().min(8))
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    io_sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Baseline {
    results: Vec<Violation>,
}

#[derive(Debug, Default)]
struct FileScan {
    relative: String,
    violations: Vec<Violation>,
    errors: Vec<ScanError>,
    imports: Vec<ImportRecord>,
    executed: BTreeSet<String>,
    partial: BTreeSet<String>,
    not_applicable: BTreeSet<String>,
    line_scope: LineScope,
    proves: Vec<(usize, String)>,
    python_package: Option<(String, String)>,
    shapes: Vec<FunctionShape>,
    facts: Vec<FunctionFact>,
}

pub fn run(cli: Cli) -> u8 {
    let (action, args) = match cli.command {
        TopCommand::Style(style) => match style.command {
            StyleCommand::Fix(args) => (StyleCommand::Fix(args.clone()), args),
            StyleCommand::Check(args) => (StyleCommand::Check(args.clone()), args),
            StyleCommand::Report(args) => (StyleCommand::Report(args.clone()), args),
        },
        TopCommand::Bench(args) => {
            return match crate::bench::run(args) {
                Ok(code) => code,
                Err(error) => {
                    eprintln!("bits bench: {error}");
                    2
                }
            };
        }
    };
    match execute(action, &args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("bits: {error}");
            2
        }
    }
}

fn execute(action: StyleCommand, args: &ScanArgs) -> Result<u8, String> {
    if args.jobs == 0 {
        return Err("--jobs must be greater than zero".to_owned());
    }
    let config = load_config(args.config.as_deref())?;
    let scope = if args.diff {
        Scope::Diff(args.commit.clone().unwrap_or_else(|| "HEAD".to_owned()))
    } else if args.changed {
        Scope::Changed
    } else if args.staged {
        Scope::Staged
    } else {
        Scope::All
    };
    let inputs = if args.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        args.paths.clone()
    };
    let discovery = discovery::discover(&inputs, scope, args.no_ignore, &config.exclude)?;
    let policy = IoPolicy::new(&config.io_sources)?;
    let diff_base = args.diff.then(|| {
        (
            discovery.root.clone(),
            args.commit.clone().unwrap_or_else(|| "HEAD".to_owned()),
        )
    });
    let files = discovery.files;
    let unsupported = discovery.unsupported;
    let dependency_context = discovery.dependency_context;
    let is_fix = matches!(action, StyleCommand::Fix(_));
    let is_report = matches!(action, StyleCommand::Report(_));

    let mut report = if is_fix {
        let before = scan_report(
            &files,
            &dependency_context,
            args.jobs,
            false,
            &unsupported,
            &policy,
            diff_base.as_ref(),
        );
        let mut fixes = apply_fixes(&files)?;
        let mut after = scan_report(
            &files,
            &dependency_context,
            args.jobs,
            args.include_passed,
            &unsupported,
            &policy,
            diff_base.as_ref(),
        );
        for fix in &mut fixes {
            fix.remaining_violations = after
                .results
                .iter()
                .filter(|violation| violation.file == fix.file)
                .count();
        }
        after.summary.files_fixed = fixes.iter().filter(|fix| fix.fixed).count();
        after.summary.violations_fixed = before
            .summary
            .fixable_violations
            .saturating_sub(after.summary.fixable_violations);
        after.fixes = fixes;
        after.finish();
        after
    } else {
        scan_report(
            &files,
            &dependency_context,
            args.jobs,
            args.include_passed,
            &unsupported,
            &policy,
            diff_base.as_ref(),
        )
    };
    if let Some(path) = &args.baseline {
        let baseline = load_baseline(path)?;
        apply_baseline(&mut report, &baseline);
    }
    for fix in &mut report.fixes {
        fix.remaining_violations = report
            .results
            .iter()
            .filter(|violation| violation.file == fix.file)
            .count();
    }
    report.finish();

    let format = args.format.unwrap_or(if is_report {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    });
    let rendered = report::render(&report, format, args.include_passed)?;
    if let Some(path) = &args.output {
        std::fs::write(path, rendered).map_err(|error| format!("{}: {error}", path.display()))?;
    } else {
        std::io::stdout()
            .write_all(rendered.as_bytes())
            .map_err(|error| error.to_string())?;
    }

    if !report.scan_errors.is_empty() {
        Ok(2)
    } else if is_report {
        Ok(0)
    } else if report.summary.unsupported_files > 0 {
        Ok(2)
    } else if report.summary.violations == 0 {
        Ok(0)
    } else {
        Ok(1)
    }
}

fn load_config(path: Option<&Path>) -> Result<Config, String> {
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let source =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn load_baseline(path: &Path) -> Result<Baseline, String> {
    let source =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn apply_baseline(report: &mut Report, baseline: &Baseline) {
    let files: BTreeSet<&str> = baseline
        .results
        .iter()
        .map(|result| result.file.as_str())
        .collect();
    let mut fingerprints: BTreeMap<(String, String, String, String), usize> = BTreeMap::new();
    for result in &baseline.results {
        *fingerprints
            .entry((
                result.rule_id.clone(),
                result.file.clone(),
                result.message.clone(),
                result.actual.to_string(),
            ))
            .or_default() += 1;
    }
    let before = report.results.len();
    report.results.retain(|result| {
        if !files.contains(result.file.as_str()) {
            return true;
        }
        let key = (
            result.rule_id.clone(),
            result.file.clone(),
            result.message.clone(),
            result.actual.to_string(),
        );
        match fingerprints.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        }
    });
    report.summary.suppressed_violations = before - report.results.len();
}

fn scan_report(
    files: &[DiscoveredFile],
    dependency_context: &[DiscoveredFile],
    jobs: usize,
    include_passed: bool,
    unsupported: &[String],
    policy: &IoPolicy,
    diff_base: Option<&(PathBuf, String)>,
) -> Report {
    let diff_mode = diff_base.is_some();
    let scans = scan_files(files, jobs, policy);
    let dependency_scans = scan_dependency_context(&scans, dependency_context, policy);
    let mut report = Report::new();
    report.summary.files_discovered = files.len();
    report.summary.files_scanned = files.len();
    report.summary.unsupported_files = unsupported.len();
    report.unsupported = unsupported.to_vec();
    let mut passed_candidates = BTreeMap::new();

    for scan in &scans {
        report.results.extend(scan.violations.clone());
        report.scan_errors.extend(scan.errors.clone());
        report
            .coverage
            .executed
            .extend(scan.executed.iter().cloned());
        report.coverage.partial.extend(scan.partial.iter().cloned());
        report
            .coverage
            .not_applicable
            .extend(scan.not_applicable.iter().cloned());
        if scan.errors.is_empty() {
            passed_candidates.insert(scan.relative.clone(), scan.executed.clone());
        }
    }
    if !passed_candidates.is_empty() {
        report
            .coverage
            .executed
            .extend(["BCS001", "BCS002", "BCS015", "BCS017"].map(str::to_owned));
    }
    check_dependency_cycles(&scans, &dependency_scans, diff_mode, &mut report.results);
    check_purity_contracts(&scans, &dependency_scans, &mut report.results);
    filter_diff_violations(&scans, diff_mode, &mut report.results);
    apply_improving_ratchet(files, &scans, diff_base, &mut report.results);
    enforce_prove_budget(&scans, diff_mode, &mut report.results);
    report.finish();

    if include_passed {
        for (file, rules) in passed_candidates {
            for rule in rules {
                if !report
                    .results
                    .iter()
                    .any(|violation| violation.file == file && violation.rule_id == rule)
                {
                    report.passed.push(PassedCheck {
                        rule_id: rule,
                        file: file.clone(),
                        status: "passed".to_owned(),
                    });
                }
            }
        }
        report
            .passed
            .sort_by(|a, b| (&a.file, &a.rule_id).cmp(&(&b.file, &b.rule_id)));
    }
    report
}

fn scan_files(files: &[DiscoveredFile], jobs: usize, policy: &IoPolicy) -> Vec<FileScan> {
    if jobs <= 1 || files.len() < 2 {
        return files.iter().map(|file| scan_file(file, policy)).collect();
    }
    let chunk_size = files.len().div_ceil(jobs);
    let mut output = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in files.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|file| scan_file(file, policy))
                    .collect::<Vec<_>>()
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(mut scans) => output.append(&mut scans),
                Err(_) => output.push(FileScan {
                    relative: "<scanner>".to_owned(),
                    errors: vec![ScanError {
                        file: "<scanner>".to_owned(),
                        error: "worker_panic".to_owned(),
                        message: "scan worker panicked".to_owned(),
                    }],
                    ..FileScan::default()
                }),
            }
        }
    });
    output
}

fn scan_dependency_context(
    scans: &[FileScan],
    files: &[DiscoveredFile],
    policy: &IoPolicy,
) -> Vec<FileScan> {
    let context: BTreeMap<&str, &DiscoveredFile> = files
        .iter()
        .map(|file| (file.relative.as_str(), file))
        .collect();
    let known: BTreeSet<&str> = scans
        .iter()
        .map(|scan| scan.relative.as_str())
        .chain(context.keys().copied())
        .collect();
    let mut visited: BTreeSet<String> = scans.iter().map(|scan| scan.relative.clone()).collect();
    let mut pending = Vec::new();
    for scan in scans {
        for import in &scan.imports {
            let Some(specifier) = analyze::dependency_path(import) else {
                continue;
            };
            if let Some(target) = resolve_dependency(
                &scan.relative,
                &specifier,
                scan.python_package.as_ref(),
                &known,
            ) {
                pending.push(target.to_owned());
            }
        }
    }

    let mut output = Vec::new();
    while let Some(relative) = pending.pop() {
        if !visited.insert(relative.clone()) {
            continue;
        }
        let Some(file) = context.get(relative.as_str()) else {
            continue;
        };
        let Some((imports, facts)) = scan_context_file(file, policy) else {
            continue;
        };
        let python_package = python_package_context(file);
        for import in &imports {
            let Some(specifier) = analyze::dependency_path(import) else {
                continue;
            };
            if let Some(target) =
                resolve_dependency(&relative, &specifier, python_package.as_ref(), &known)
            {
                pending.push(target.to_owned());
            }
        }
        output.push(FileScan {
            python_package,
            relative,
            imports,
            facts,
            ..FileScan::default()
        });
    }
    output
}

fn scan_context_file(
    file: &DiscoveredFile,
    policy: &IoPolicy,
) -> Option<(Vec<ImportRecord>, Vec<FunctionFact>)> {
    let source = std::fs::read_to_string(&file.absolute).ok()?;
    let tree = file.language.parse(&source).ok()?;
    if tree.root_node().has_error() {
        return None;
    }
    let imports = analyze::analyze_imports(file.language, &source, &tree);
    let facts = taint::extract_facts(
        file.language,
        &file.relative,
        &source,
        &tree,
        &imports,
        policy,
    );
    Some((imports, facts))
}

fn scan_file(file: &DiscoveredFile, policy: &IoPolicy) -> FileScan {
    let mut scan = FileScan {
        python_package: python_package_context(file),
        relative: file.relative.clone(),
        line_scope: file.line_scope.clone(),
        ..FileScan::default()
    };
    let source = match std::fs::read_to_string(&file.absolute) {
        Ok(source) => source,
        Err(error) => {
            scan.errors.push(ScanError {
                file: file.relative.clone(),
                error: "read_error".to_owned(),
                message: error.to_string(),
            });
            return scan;
        }
    };
    let tree = match file.language.parse(&source) {
        Ok(tree) => tree,
        Err(error) => {
            scan.errors.push(ScanError {
                file: file.relative.clone(),
                error: "parser_error".to_owned(),
                message: error,
            });
            return scan;
        }
    };
    if tree.root_node().has_error() {
        let location = crate::language::first_error_position(&tree)
            .map(|(line, column)| format!(" at {line}:{column}"))
            .unwrap_or_default();
        scan.errors.push(ScanError {
            file: file.relative.clone(),
            error: "parse_error".to_owned(),
            message: format!(
                "{} syntax tree contains errors{location}",
                file.language.name()
            ),
        });
        return scan;
    }
    let analysis = analyze::analyze(file.language, &file.relative, &source, &tree);
    scan.violations.extend(text::scan(
        file.language,
        &file.relative,
        &source,
        &analysis.strings,
        &analysis.comments,
    ));
    scan.shapes = analyze::function_shapes(file.language, &source, &tree);
    let comments = analysis.comments.clone();
    merge_analysis(&mut scan, analysis);
    scan.facts = taint::extract_facts(
        file.language,
        &file.relative,
        &source,
        &tree,
        &scan.imports,
        policy,
    );
    scan.proves =
        text::apply_inline_suppressions(&file.relative, &source, &comments, &mut scan.violations);
    scan
}

pub fn scan_path_for_bench(path: &Path, exclude: &[String]) -> Result<Report, String> {
    let discovery = discovery::discover(&[path.to_path_buf()], Scope::All, false, exclude)?;
    let policy = IoPolicy::new(&[])?;
    Ok(scan_report(
        &discovery.files,
        &[],
        default_jobs(),
        false,
        &discovery.unsupported,
        &policy,
        None,
    ))
}

fn check_purity_contracts(
    scans: &[FileScan],
    context: &[FileScan],
    violations: &mut Vec<Violation>,
) {
    let all: Vec<&FileScan> = scans.iter().chain(context.iter()).collect();
    let known: BTreeSet<&str> = all.iter().map(|scan| scan.relative.as_str()).collect();
    let by_file: BTreeMap<&str, &FileScan> = all
        .iter()
        .map(|scan| (scan.relative.as_str(), *scan))
        .collect();
    let resolve_import = |file: &str, symbol: &str| -> Option<(String, String)> {
        let scan = by_file.get(file)?;
        for import in &scan.imports {
            if import.binding.as_deref() != Some(symbol) {
                continue;
            }
            let specifier = analyze::dependency_path(import)?;
            let target =
                resolve_dependency(file, &specifier, scan.python_package.as_ref(), &known)?;
            return Some((target.to_owned(), symbol.to_owned()));
        }
        None
    };
    let facts: Vec<FunctionFact> = all
        .iter()
        .flat_map(|scan| scan.facts.iter().cloned())
        .collect();
    violations.extend(taint::check_purity(&facts, &resolve_import));
}

fn apply_improving_ratchet(
    files: &[DiscoveredFile],
    scans: &[FileScan],
    diff_base: Option<&(PathBuf, String)>,
    violations: &mut [Violation],
) {
    let Some((root, base)) = diff_base else {
        return;
    };
    let languages: BTreeMap<&str, Language> = files
        .iter()
        .map(|file| (file.relative.as_str(), file.language))
        .collect();
    let by_file: BTreeMap<&str, &FileScan> = scans
        .iter()
        .map(|scan| (scan.relative.as_str(), scan))
        .collect();
    let mut cache: BTreeMap<String, Option<BTreeMap<String, FunctionShape>>> = BTreeMap::new();
    for violation in violations.iter_mut() {
        let Some(&language) = languages.get(violation.file.as_str()) else {
            continue;
        };
        let Some(scan) = by_file.get(violation.file.as_str()) else {
            continue;
        };
        let base_shapes = cache
            .entry(violation.file.clone())
            .or_insert_with(|| load_base_shapes(root, base, &violation.file, language));
        let Some(base_shapes) = base_shapes else {
            continue;
        };
        if ratchet_improved(violation, scan, base_shapes) {
            violation.improving = true;
            violation.severity = "warning".to_owned();
            violation.obligation = Some("ratchet_improving".to_owned());
        }
    }
}

fn ratchet_improved(
    violation: &Violation,
    scan: &FileScan,
    base_shapes: &BTreeMap<String, FunctionShape>,
) -> bool {
    if violation.rule_id != "BCS003" {
        return false;
    }
    let line = violation.range.start.line;
    let Some(current) = scan
        .shapes
        .iter()
        .find(|shape| shape.start_line <= line && shape.end_line >= line)
    else {
        return false;
    };
    let Some(previous) = base_shapes.get(&current.name) else {
        return false;
    };
    match violation.message.as_str() {
        "control-flow nesting exceeds three levels" => current.nesting < previous.nesting,
        "cyclomatic complexity exceeds ten" => current.complexity < previous.complexity,
        _ => false,
    }
}

fn load_base_shapes(
    root: &Path,
    base: &str,
    relative: &str,
    language: Language,
) -> Option<BTreeMap<String, FunctionShape>> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "core.quotePath=false",
            "show",
            "--no-ext-diff",
            "--no-textconv",
            &format!("{base}:./{relative}"),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let source = String::from_utf8(output.stdout).ok()?;
    let tree = language.parse(&source).ok()?;
    if tree.root_node().has_error() {
        return None;
    }
    Some(
        analyze::function_shapes(language, &source, &tree)
            .into_iter()
            .map(|shape| (shape.name.clone(), shape))
            .collect(),
    )
}

fn enforce_prove_budget(scans: &[FileScan], diff_mode: bool, violations: &mut Vec<Violation>) {
    if !diff_mode {
        return;
    }
    const PROVE_BUDGET: usize = 1;
    let mut added = Vec::new();
    for scan in scans {
        for (line, rule) in &scan.proves {
            let is_new = match &scan.line_scope {
                LineScope::Added(lines) => lines.contains(line),
                LineScope::WholeFile => true,
            };
            if is_new {
                added.push((scan.relative.as_str(), *line, rule.as_str()));
            }
        }
    }
    added.sort_unstable();
    for (file, line, rule) in added.into_iter().skip(PROVE_BUDGET) {
        violations.push(Violation::new(
            "BCS015",
            file,
            line,
            1,
            "new bits:prove annotations exceed the per-change budget of 1",
            format!("bits:prove({rule})"),
            "<=1 new bits:prove per change",
            false,
        ));
    }
}

fn merge_analysis(scan: &mut FileScan, analysis: SyntaxAnalysis) {
    scan.violations.extend(analysis.violations);
    scan.imports.extend(analysis.imports);
    scan.executed.extend(analysis.executed);
    scan.partial.extend(analysis.partial);
    scan.not_applicable.extend(analysis.not_applicable);
}

fn apply_fixes(files: &[DiscoveredFile]) -> Result<Vec<FileFix>, String> {
    let mut records = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file.absolute).map_err(|error| error.to_string())?;
        let tree = match file.language.parse(&source) {
            Ok(tree) if !tree.root_node().has_error() => tree,
            _ => continue,
        };
        let analysis = analyze::analyze(file.language, &file.relative, &source, &tree);
        let (fixed, applied_fixes) =
            text::fix(&source, &analysis.strings, file.language, &file.line_scope);
        let changed = fixed != source;
        if changed {
            std::fs::write(&file.absolute, fixed).map_err(|error| error.to_string())?;
        }
        records.push(FileFix {
            file: file.relative.clone(),
            fixed: changed,
            applied_fixes,
            remaining_violations: 0,
        });
    }
    Ok(records)
}

fn filter_diff_violations(scans: &[FileScan], diff_mode: bool, violations: &mut Vec<Violation>) {
    if !diff_mode {
        return;
    }
    let scopes: BTreeMap<&str, &LineScope> = scans
        .iter()
        .map(|scan| (scan.relative.as_str(), &scan.line_scope))
        .collect();
    violations.retain(|violation| match scopes.get(violation.file.as_str()) {
        Some(LineScope::Added(lines)) => lines
            .range(violation.range.start.line..=violation.range.end.line)
            .next()
            .is_some(),
        Some(LineScope::WholeFile) => true,
        None => false,
    });
}

fn check_dependency_cycles(
    scans: &[FileScan],
    context: &[FileScan],
    diff_mode: bool,
    violations: &mut Vec<Violation>,
) {
    let all: Vec<&FileScan> = scans.iter().chain(context.iter()).collect();
    let known: BTreeSet<&str> = all.iter().map(|scan| scan.relative.as_str()).collect();
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for scan in all {
        for import in &scan.imports {
            if import.type_only {
                continue;
            }
            let Some(specifier) = analyze::dependency_path(import) else {
                continue;
            };
            if let Some(target) = resolve_dependency(
                &scan.relative,
                &specifier,
                scan.python_package.as_ref(),
                &known,
            ) {
                if target == scan.relative {
                    continue;
                }
                graph.entry(&scan.relative).or_default().push(target);
                edges.insert((scan.relative.as_str(), target), import);
            }
        }
    }

    if diff_mode {
        report_diff_cycles(scans, &graph, &edges, violations);
        return;
    }

    let mut search = CycleSearch {
        state: BTreeMap::new(),
        stack: Vec::new(),
        emitted: BTreeSet::new(),
    };
    for scan in scans {
        visit_cycle(&scan.relative, &graph, &edges, &mut search, violations);
    }
}

fn report_diff_cycles<'a>(
    scans: &[FileScan],
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    edges: &BTreeMap<(&'a str, &'a str), &ImportRecord>,
    violations: &mut Vec<Violation>,
) {
    let changed: BTreeSet<&str> = scans.iter().map(|scan| scan.relative.as_str()).collect();
    for ((from, to), import) in edges {
        let from = *from;
        let to = *to;
        if changed.contains(from) && dependency_reaches(graph, to, from) {
            violations.push(Violation::new(
                "BCS014",
                from,
                import.line,
                import.column,
                "dependency cycle detected",
                format!("{from} -> {to}"),
                "acyclic_dependencies",
                false,
            ));
        }
    }
}

fn dependency_reaches<'a>(
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    start: &'a str,
    target: &'a str,
) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if !visited.insert(node) {
            continue;
        }
        if let Some(targets) = graph.get(node) {
            pending.extend(targets.iter().copied());
        }
    }
    false
}

struct CycleSearch<'a> {
    state: BTreeMap<&'a str, usize>,
    stack: Vec<&'a str>,
    emitted: BTreeSet<String>,
}

fn visit_cycle<'a>(
    file: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    edges: &BTreeMap<(&'a str, &'a str), &ImportRecord>,
    search: &mut CycleSearch<'a>,
    violations: &mut Vec<Violation>,
) {
    if search.state.get(file) == Some(&2) {
        return;
    }
    if search.state.get(file) == Some(&1) {
        let Some(start) = search.stack.iter().position(|entry| *entry == file) else {
            return;
        };
        let mut cycle = search.stack[start..].to_vec();
        cycle.sort();
        let key = cycle.join("->");
        if search.emitted.insert(key) {
            let from = search.stack.last().copied().unwrap_or(file);
            if let Some(import) = edges.get(&(from, file)) {
                violations.push(Violation::new(
                    "BCS014",
                    from,
                    import.line,
                    import.column,
                    "dependency cycle detected",
                    format!("{from} -> {file}"),
                    "acyclic_dependencies",
                    false,
                ));
            }
        }
        return;
    }
    search.state.insert(file, 1);
    search.stack.push(file);
    if let Some(targets) = graph.get(file) {
        for target in targets {
            visit_cycle(target, graph, edges, search, violations);
        }
    }
    search.stack.pop();
    search.state.insert(file, 2);
}

fn python_package_context(file: &DiscoveredFile) -> Option<(String, String)> {
    if file.language != crate::language::Language::Python {
        return None;
    }
    let mut abs_dir = file.absolute.parent()?;
    let mut rel_dir = Path::new(&file.relative).parent().unwrap_or(Path::new(""));
    let mut pkg_name = None;
    let mut pkg_dir = rel_dir;
    while abs_dir.join("__init__.py").is_file() || abs_dir.join("__init__.pyi").is_file() {
        pkg_name = abs_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        pkg_dir = rel_dir;
        rel_dir = rel_dir.parent().unwrap_or(Path::new(""));
        abs_dir = abs_dir.parent()?;
    }
    Some((pkg_name?, pkg_dir.to_string_lossy().replace('\\', "/")))
}

fn resolve_dependency<'a>(
    current: &str,
    specifier: &str,
    python_package: Option<&(String, String)>,
    known: &BTreeSet<&'a str>,
) -> Option<&'a str> {
    let parent = Path::new(current).parent().unwrap_or(Path::new(""));
    if let Some(absolute) = specifier.strip_prefix('/') {
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
        return resolve_module_base(&base, known);
    }
    let base = normalize_path(&parent.join(specifier))
        .to_string_lossy()
        .replace('\\', "/");
    resolve_module_base(&base, known)
}

fn resolve_module_base<'a>(base: &str, known: &BTreeSet<&'a str>) -> Option<&'a str> {
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

pub fn rule_ids() -> &'static [&'static str] {
    &RULE_IDS
}
