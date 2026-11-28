mod baseline;
mod deps;
mod pipeline;
mod policy;
mod resolve;
mod worker;

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::analyze::{FunctionShape, ImportRecord};
use crate::discovery::{self, Discovery, LineScope, Scope};
use crate::layers::LayerPolicy;
use crate::model::{
    AllowAnnotation, FileFix, RULE_IDS, Report, ScanError, Violation,
};
use crate::report::{self, OutputFormat};
use crate::taint::{FunctionFact, IoPolicy};
use crate::text::MatchedProve;

use baseline::{
    apply_baseline, check_min_version, execute_baseline_prune, load_baseline,
    load_config,
};
use pipeline::{DiffBase, ScanEnv, scan_report};
use worker::apply_fixes;

pub use pipeline::{BenchScan, scan_path_for_bench};

#[derive(Debug, Args)]
pub struct StyleArgs {
    #[command(subcommand)]
    command: StyleCommand,
}

#[derive(Debug, Subcommand)]
enum StyleCommand {
    Fix(ScanArgs),
    Check(ScanArgs),
    Report(ScanArgs),
    Baseline(BaselineArgs),
}

#[derive(Clone, Debug, Args)]
struct BaselineArgs {
    #[arg(long, value_name = "FILE")]
    prune: PathBuf,
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,
    #[arg(long, default_value_t = default_jobs())]
    jobs: usize,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    no_ignore: bool,
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
    #[arg(long, conflicts_with = "format")]
    json: bool,
    #[arg(long)]
    violations_only: bool,
    #[arg(long)]
    strict_unsupported: bool,
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

pub(super) fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |parallelism| parallelism.get().min(8))
}

#[derive(Debug, Default)]
pub(super) struct FileScan {
    pub relative: String,
    pub violations: Vec<Violation>,
    pub errors: Vec<ScanError>,
    pub imports: Vec<ImportRecord>,
    pub executed: BTreeSet<String>,
    pub partial: BTreeSet<String>,
    pub not_applicable: BTreeSet<String>,
    pub line_scope: LineScope,
    pub proves: Vec<MatchedProve>,
    pub allows: Vec<AllowAnnotation>,
    pub legacy_explains: Vec<usize>,
    pub tests: Vec<String>,
    pub python_package: Option<(String, String)>,
    pub shapes: Vec<FunctionShape>,
    pub facts: Vec<FunctionFact>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Fix,
    Check,
    Report,
}

pub fn run_style(style: StyleArgs) -> u8 {
    let result = match style.command {
        StyleCommand::Fix(args) => execute(RunMode::Fix, &args),
        StyleCommand::Check(args) => execute(RunMode::Check, &args),
        StyleCommand::Report(args) => execute(RunMode::Report, &args),
        StyleCommand::Baseline(args) => execute_baseline_prune(&args),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("bits: {error}");
            2
        }
    }
}

fn execute(mode: RunMode, args: &ScanArgs) -> Result<u8, String> {
    if args.jobs == 0 {
        return Err("--jobs must be greater than zero".to_owned());
    }
    let config = load_config(args.config.as_deref())?;
    check_min_version(&config)?;
    let discovery = discover_inputs(args, &config.exclude)?;
    let policy = IoPolicy::new(&config.io_sources)?;
    let layers = LayerPolicy::compile(&config.layers)?;
    let diff_base = resolve_diff_base(args, &discovery.root);
    let env = ScanEnv {
        jobs: args.jobs,
        include_passed: args.include_passed,
        policy: &policy,
        layers: &layers,
        diff_base: diff_base.as_ref(),
    };
    let mut report = run_scan_mode(mode, &discovery, &env)?;
    finalize_report(&mut report, args, &discovery)?;
    emit_output(&report, mode, args)?;
    Ok(exit_code(&report, mode))
}

fn discover_inputs(
    args: &ScanArgs,
    exclude: &[String],
) -> Result<Discovery, String> {
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
    discovery::discover(&inputs, scope, args.no_ignore, exclude)
}

fn resolve_diff_base(args: &ScanArgs, root: &Path) -> Option<DiffBase> {
    args.diff.then(|| DiffBase {
        root: root.to_path_buf(),
        commit: args.commit.clone().unwrap_or_else(|| "HEAD".to_owned()),
    })
}

fn run_scan_mode(
    mode: RunMode,
    discovery: &Discovery,
    env: &ScanEnv<'_>,
) -> Result<Report, String> {
    if mode != RunMode::Fix {
        return Ok(scan_report(discovery, env));
    }
    let before_env = ScanEnv {
        include_passed: false,
        ..*env
    };
    let before = scan_report(discovery, &before_env);
    let mut fixes = apply_fixes(&discovery.files)?;
    let mut after = scan_report(discovery, env);
    update_fix_remaining(&mut fixes, &after.results);
    after.summary.files_fixed = fixes.iter().filter(|fix| fix.fixed).count();
    after.summary.violations_fixed = before
        .summary
        .fixable_violations
        .saturating_sub(after.summary.fixable_violations);
    after.fixes = fixes;
    after.finish();
    Ok(after)
}

fn update_fix_remaining(fixes: &mut [FileFix], results: &[Violation]) {
    for fix in fixes {
        fix.remaining_violations = results
            .iter()
            .filter(|violation| violation.file == fix.file)
            .count();
    }
}

fn finalize_report(
    report: &mut Report,
    args: &ScanArgs,
    discovery: &Discovery,
) -> Result<(), String> {
    report.fail_on_unsupported = (args.strict_unsupported
        && !discovery.unsupported.is_empty())
        || !discovery.explicit_unsupported.is_empty();
    if let Some(path) = &args.baseline {
        let baseline = load_baseline(path)?;
        apply_baseline(report, &baseline);
    }
    update_fix_remaining(&mut report.fixes, &report.results);
    report.finish();
    Ok(())
}

fn emit_output(
    report: &Report,
    mode: RunMode,
    args: &ScanArgs,
) -> Result<(), String> {
    let default_fmt = if mode == RunMode::Report {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    };
    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format.unwrap_or(default_fmt)
    };
    let rendered = report::render(
        report,
        format,
        args.include_passed,
        args.violations_only,
    )?;
    if let Some(path) = &args.output {
        std::fs::write(path, rendered)
            .map_err(|error| format!("{}: {error}", path.display()))
    } else {
        std::io::stdout()
            .write_all(rendered.as_bytes())
            .map_err(|error| error.to_string())
    }
}

fn exit_code(report: &Report, mode: RunMode) -> u8 {
    if !report.scan_errors.is_empty() {
        2
    } else if mode == RunMode::Report {
        0
    } else if report.fail_on_unsupported {
        2
    } else if report.summary.violations == 0 {
        0
    } else {
        1
    }
}

pub fn rule_ids() -> &'static [&'static str] {
    &RULE_IDS
}
