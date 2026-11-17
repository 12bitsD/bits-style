use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use crate::analyze::{self, ImportRecord, SyntaxAnalysis};
use crate::discovery::{self, DiscoveredFile, LineScope, Scope};
use crate::model::{FileFix, PassedCheck, RULE_IDS, Report, ScanError, Violation};
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
}

pub fn run(cli: Cli) -> u8 {
    let (action, args) = match cli.command {
        TopCommand::Style(style) => match style.command {
            StyleCommand::Fix(args) => (StyleCommand::Fix(args.clone()), args),
            StyleCommand::Check(args) => (StyleCommand::Check(args.clone()), args),
            StyleCommand::Report(args) => (StyleCommand::Report(args.clone()), args),
        },
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
    let files = discovery.files;
    let unsupported = discovery.unsupported;
    let is_fix = matches!(action, StyleCommand::Fix(_));
    let is_report = matches!(action, StyleCommand::Report(_));

    let mut report = if is_fix {
        let before = scan_report(&files, args.jobs, false, &unsupported);
        let mut fixes = apply_fixes(&files)?;
        let mut after = scan_report(&files, args.jobs, args.include_passed, &unsupported);
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
        scan_report(&files, args.jobs, args.include_passed, &unsupported)
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
    jobs: usize,
    include_passed: bool,
    unsupported: &[String],
) -> Report {
    let scans = scan_files(files, jobs);
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
            .extend(["BCS001", "BCS002", "BCS015"].map(str::to_owned));
    }
    check_dependency_cycles(&scans, &mut report.results);
    filter_diff_violations(&scans, &mut report.results);
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

fn scan_files(files: &[DiscoveredFile], jobs: usize) -> Vec<FileScan> {
    if jobs <= 1 || files.len() < 2 {
        return files.iter().map(scan_file).collect();
    }
    let chunk_size = files.len().div_ceil(jobs);
    let mut output = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in files.chunks(chunk_size) {
            handles.push(scope.spawn(move || chunk.iter().map(scan_file).collect::<Vec<_>>()));
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

fn scan_file(file: &DiscoveredFile) -> FileScan {
    let mut scan = FileScan {
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
    let comments = analysis.comments.clone();
    merge_analysis(&mut scan, analysis);
    text::apply_inline_suppressions(&file.relative, &source, &comments, &mut scan.violations);
    scan
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

fn filter_diff_violations(scans: &[FileScan], violations: &mut Vec<Violation>) {
    let scopes: BTreeMap<&str, &LineScope> = scans
        .iter()
        .map(|scan| (scan.relative.as_str(), &scan.line_scope))
        .collect();
    let has_diff = scans
        .iter()
        .any(|scan| matches!(scan.line_scope, LineScope::Added(_)));
    violations.retain(|violation| match scopes.get(violation.file.as_str()) {
        Some(LineScope::Added(lines)) => lines
            .range(violation.range.start.line..=violation.range.end.line)
            .next()
            .is_some(),
        Some(LineScope::WholeFile) => true,
        None => !has_diff,
    });
}

fn check_dependency_cycles(scans: &[FileScan], violations: &mut Vec<Violation>) {
    let known: BTreeSet<&str> = scans.iter().map(|scan| scan.relative.as_str()).collect();
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for scan in scans {
        for import in &scan.imports {
            let Some(specifier) = analyze::dependency_path(import) else {
                continue;
            };
            if let Some(target) = resolve_dependency(&scan.relative, &specifier, &known) {
                graph.entry(&scan.relative).or_default().push(target);
                edges.insert((scan.relative.as_str(), target), import);
            }
        }
    }

    let mut state = BTreeMap::new();
    let mut stack = Vec::new();
    let mut emitted = BTreeSet::new();
    for scan in scans {
        visit_cycle(
            &scan.relative,
            &graph,
            &edges,
            &mut state,
            &mut stack,
            &mut emitted,
            violations,
        );
    }
}

fn visit_cycle<'a>(
    file: &'a str,
    graph: &BTreeMap<&'a str, Vec<&'a str>>,
    edges: &BTreeMap<(&'a str, &'a str), &ImportRecord>,
    state: &mut BTreeMap<&'a str, usize>,
    stack: &mut Vec<&'a str>,
    emitted: &mut BTreeSet<String>,
    violations: &mut Vec<Violation>,
) {
    if state.get(file) == Some(&2) {
        return;
    }
    if state.get(file) == Some(&1) {
        let Some(start) = stack.iter().position(|entry| *entry == file) else {
            return;
        };
        let mut cycle = stack[start..].to_vec();
        cycle.sort();
        let key = cycle.join("->");
        if emitted.insert(key) {
            let from = stack.last().copied().unwrap_or(file);
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
    state.insert(file, 1);
    stack.push(file);
    if let Some(targets) = graph.get(file) {
        for target in targets {
            visit_cycle(target, graph, edges, state, stack, emitted, violations);
        }
    }
    stack.pop();
    state.insert(file, 2);
}

fn resolve_dependency<'a>(
    current: &str,
    specifier: &str,
    known: &BTreeSet<&'a str>,
) -> Option<&'a str> {
    let parent = Path::new(current).parent().unwrap_or(Path::new(""));
    let joined = normalize_path(&parent.join(specifier));
    let base = joined.to_string_lossy().replace('\\', "/");
    let candidates = if Path::new(&base).extension().is_some() {
        vec![base.clone()]
    } else {
        vec![
            format!("{base}.ts"),
            format!("{base}.tsx"),
            format!("{base}.js"),
            format!("{base}.jsx"),
            format!("{base}.py"),
            format!("{base}/index.ts"),
            format!("{base}/index.js"),
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

pub fn rule_ids() -> &'static [&'static str; 15] {
    &RULE_IDS
}
