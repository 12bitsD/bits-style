use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::discovery::{self, DiscoveredFile, Discovery, Scope};
use crate::layers::LayerPolicy;
use crate::model::{PassedCheck, Report, ScanError, Violation, is_format_rule};
use crate::taint::IoPolicy;

use super::deps::{
    check_dependency_cycles, check_layer_boundaries, check_purity_contracts,
};
use super::policy::{
    ScanBundle, apply_improving_ratchet, enforce_annotation_policy,
    filter_cross_file_imported_private_symbols, filter_diff_violations,
    is_added_line,
};
use super::worker::{scan_dependency_context, scan_file};
use super::{FileScan, default_jobs};

pub(super) struct DiffBase {
    pub root: PathBuf,
    pub commit: String,
}

pub(super) struct ScanEnv<'a> {
    pub jobs: usize,
    pub include_passed: bool,
    pub policy: &'a IoPolicy,
    pub layers: &'a LayerPolicy,
    pub diff_base: Option<&'a DiffBase>,
}

pub struct BenchScan {
    pub report: Report,
    pub cognitive_scores: Vec<usize>,
    pub complexity_pairs: Vec<(usize, usize)>,
    pub functions_with_warnings: usize,
}

pub fn scan_path_for_bench(
    path: &Path,
    exclude: &[String],
) -> Result<BenchScan, String> {
    let mut discovery =
        discovery::discover(&[path.to_path_buf()], Scope::All, false, exclude)?;
    discovery.dependency_context.clear();
    let policy = IoPolicy::new(&[])?;
    let layers = LayerPolicy::default();
    let env = ScanEnv {
        jobs: default_jobs(),
        include_passed: false,
        policy: &policy,
        layers: &layers,
        diff_base: None,
    };
    let scans = scan_files(&discovery.files, &env);
    let cognitive_scores = scans
        .iter()
        .flat_map(|scan| scan.shapes.iter().map(|shape| shape.cognitive))
        .collect();
    let complexity_pairs = scans
        .iter()
        .flat_map(|scan| {
            scan.shapes
                .iter()
                .map(|shape| (shape.complexity, shape.cognitive))
        })
        .collect();
    let mut report = build_report(&discovery, &scans, &env);
    report
        .results
        .retain(|violation| !is_format_rule(&violation.rule_id));
    report.finish();
    report.summary.functions_with_warnings =
        count_warned_functions(&scans, &report.results);
    Ok(BenchScan {
        functions_with_warnings: report.summary.functions_with_warnings,
        report,
        cognitive_scores,
        complexity_pairs,
    })
}

pub(super) fn scan_report(discovery: &Discovery, env: &ScanEnv<'_>) -> Report {
    let scans = scan_files(&discovery.files, env);
    build_report(discovery, &scans, env)
}

fn build_report(
    discovery: &Discovery,
    scans: &[FileScan],
    env: &ScanEnv<'_>,
) -> Report {
    let diff_mode = env.diff_base.is_some();
    let dep_scans =
        scan_dependency_context(scans, &discovery.dependency_context, env);
    let mut report = init_report(discovery, scans);
    let passed_candidates = collect_file_scans(&mut report, scans, diff_mode);
    let mut bundle = ScanBundle {
        scans,
        context: &dep_scans,
        diff_mode,
        violations: &mut report.results,
    };
    check_dependency_cycles(&mut bundle);
    check_layer_boundaries(&mut bundle, env.layers);
    check_purity_contracts(&mut bundle);
    filter_cross_file_imported_private_symbols(&mut bundle);
    filter_diff_violations(&mut bundle);
    apply_improving_ratchet(
        &discovery.files,
        scans,
        env.diff_base,
        bundle.violations,
    );
    enforce_annotation_policy(&mut bundle);
    report.finish();
    report.summary.functions_with_warnings =
        count_warned_functions(scans, &report.results);
    if env.include_passed {
        append_passed_checks(&mut report, passed_candidates);
    }
    report
}

fn init_report(discovery: &Discovery, scans: &[FileScan]) -> Report {
    let mut report = Report::new();
    report.summary.files_discovered = discovery.files.len();
    report.summary.files_scanned = discovery.files.len();
    report.summary.unsupported_files = discovery.unsupported.len();
    report.summary.functions_checked =
        scans.iter().map(|scan| scan.shapes.len()).sum();
    report.unsupported = discovery.unsupported.clone();
    report
}

fn collect_file_scans(
    report: &mut Report,
    scans: &[FileScan],
    diff_mode: bool,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut passed_candidates = BTreeMap::new();
    for scan in scans {
        report.results.extend(scan.violations.clone());
        report.scan_errors.extend(scan.errors.clone());
        collect_allows(report, scan, diff_mode);
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
            passed_candidates
                .insert(scan.relative.clone(), scan.executed.clone());
        }
    }
    if !passed_candidates.is_empty() {
        report.coverage.executed.extend(
            ["BCS001", "BCS002", "BCS015", "BCS017"].map(str::to_owned),
        );
    }
    passed_candidates
}

fn collect_allows(report: &mut Report, scan: &FileScan, diff_mode: bool) {
    if diff_mode {
        report.allows.extend(
            scan.allows
                .iter()
                .filter(|allow| is_added_line(&scan.line_scope, allow.line))
                .cloned(),
        );
    } else {
        report.allows.extend(scan.allows.clone());
    }
}

fn append_passed_checks(
    report: &mut Report,
    passed_candidates: BTreeMap<String, BTreeSet<String>>,
) {
    let failed: BTreeSet<(&str, &str)> = report
        .results
        .iter()
        .map(|v| (v.file.as_str(), v.rule_id.as_str()))
        .collect();
    for (file, rules) in passed_candidates {
        for rule in rules {
            if !failed.contains(&(file.as_str(), rule.as_str())) {
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

fn scan_files(files: &[DiscoveredFile], env: &ScanEnv<'_>) -> Vec<FileScan> {
    if env.jobs <= 1 || files.len() < 2 {
        return files.iter().map(|file| scan_file(file, env)).collect();
    }
    let chunk_size = files.len().div_ceil(env.jobs);
    let mut output = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in files.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|file| scan_file(file, env))
                    .collect::<Vec<_>>()
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(mut scans) => output.append(&mut scans),
                Err(_) => output.push(worker_panic_scan()),
            }
        }
    });
    output
}

fn worker_panic_scan() -> FileScan {
    FileScan {
        relative: "<scanner>".to_owned(),
        errors: vec![ScanError {
            file: "<scanner>".to_owned(),
            error: "worker_panic".to_owned(),
            message: "scan worker panicked".to_owned(),
        }],
        ..FileScan::default()
    }
}

fn count_warned_functions(scans: &[FileScan], results: &[Violation]) -> usize {
    let warned: BTreeSet<(&str, usize)> = results
        .iter()
        .filter(|violation| violation.severity == "warning")
        .map(|violation| (violation.file.as_str(), violation.range.start.line))
        .collect();
    scans
        .iter()
        .flat_map(|scan| {
            scan.shapes.iter().filter(|shape| {
                (shape.start_line..=shape.end_line).any(|line| {
                    warned.contains(&(scan.relative.as_str(), line))
                })
            })
        })
        .count()
}
