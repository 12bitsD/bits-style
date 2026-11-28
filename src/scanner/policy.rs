use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::analyze::{self, FunctionShape};
use crate::discovery::{DiscoveredFile, LineScope};
use crate::language::Language;
use crate::model::Violation;

use super::FileScan;
use super::pipeline::DiffBase;

pub(super) struct ScanBundle<'a> {
    pub scans: &'a [FileScan],
    pub context: &'a [FileScan],
    pub diff_mode: bool,
    pub violations: &'a mut Vec<Violation>,
}

impl<'a> ScanBundle<'a> {
    pub fn all_scans(&self) -> impl Iterator<Item = &'a FileScan> {
        self.scans.iter().chain(self.context.iter())
    }
}

pub(super) fn is_added_line(scope: &LineScope, line: usize) -> bool {
    match scope {
        LineScope::Added(lines) => lines.contains(&line),
        LineScope::WholeFile => true,
    }
}

pub(super) fn filter_cross_file_imported_private_symbols(
    bundle: &mut ScanBundle<'_>,
) {
    let mut imported_bindings: BTreeSet<&str> = BTreeSet::new();
    for scan in bundle.all_scans() {
        for import in &scan.imports {
            if let Some(binding) = import.binding.as_deref()
                && binding.starts_with('_')
            {
                imported_bindings.insert(binding);
            }
        }
    }
    if imported_bindings.is_empty() {
        return;
    }
    bundle.violations.retain(|violation| {
        !(violation.rule_id == "BCS014"
            && violation.message == "unused private function"
            && violation
                .actual
                .as_str()
                .is_some_and(|name| imported_bindings.contains(name)))
    });
}

pub(super) fn filter_diff_violations(bundle: &mut ScanBundle<'_>) {
    if !bundle.diff_mode {
        return;
    }
    let scopes: BTreeMap<&str, &LineScope> = bundle
        .scans
        .iter()
        .map(|scan| (scan.relative.as_str(), &scan.line_scope))
        .collect();
    bundle.violations.retain(|violation| {
        match scopes.get(violation.file.as_str()) {
            Some(LineScope::Added(lines)) => lines
                .range(violation.range.start.line..=violation.range.end.line)
                .next()
                .is_some(),
            Some(LineScope::WholeFile) => true,
            None => false,
        }
    });
}

pub(super) fn enforce_annotation_policy(bundle: &mut ScanBundle<'_>) {
    validate_prove_tests(bundle);
    if bundle.diff_mode {
        enforce_diff_annotation_budget(bundle);
    }
}

fn validate_prove_tests(bundle: &mut ScanBundle<'_>) {
    let mut test_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for test in bundle.all_scans().flat_map(|scan| &scan.tests) {
        *test_counts.entry(test.as_str()).or_default() += 1;
    }
    for scan in bundle.scans {
        for prove in &scan.proves {
            let Some(test) = &prove.test else {
                continue;
            };
            let count = test_counts.get(test.as_str()).copied().unwrap_or(0);
            if count != 1 {
                bundle.violations.push(prove_test_violation(
                    &scan.relative,
                    prove.line,
                    test,
                    count,
                ));
            }
        }
    }
}

fn prove_test_violation(
    file: &str,
    line: usize,
    test: &str,
    count: usize,
) -> Violation {
    let message = if count == 0 {
        "bits:prove test reference does not resolve to a known test"
    } else {
        "bits:prove test reference is ambiguous"
    };
    Violation::new(
        "BCS015",
        file,
        line,
        1,
        message,
        format!("test={test}"),
        "unique_test_definition",
        false,
    )
}

fn enforce_diff_annotation_budget(bundle: &mut ScanBundle<'_>) {
    const PROVE_BUDGET: usize = 1;
    let mut added = Vec::new();
    for scan in bundle.scans {
        record_legacy_explains(scan, bundle.violations);
        for prove in &scan.proves {
            if is_added_line(&scan.line_scope, prove.line) {
                added.push((
                    scan.relative.as_str(),
                    prove.line,
                    prove.rule.as_str(),
                ));
            }
        }
    }
    added.sort_unstable();
    for (file, line, rule) in added.into_iter().skip(PROVE_BUDGET) {
        bundle.violations.push(Violation::new(
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

fn record_legacy_explains(scan: &FileScan, violations: &mut Vec<Violation>) {
    for line in &scan.legacy_explains {
        if is_added_line(&scan.line_scope, *line) {
            violations.push(Violation::new(
                "BCS015",
                &scan.relative,
                *line,
                1,
                "new bits:explain is forbidden in diff mode",
                "bits:explain",
                "bits:prove(BCS003[, test=name]): minimal reason; \
                 concrete risk",
                false,
            ));
        }
    }
}

pub(super) fn apply_improving_ratchet(
    files: &[DiscoveredFile],
    scans: &[FileScan],
    diff_base: Option<&DiffBase>,
    violations: &mut [Violation],
) {
    let Some(diff_base) = diff_base else {
        return;
    };
    let root = &diff_base.root;
    let base = &diff_base.commit;
    let languages: BTreeMap<&str, Language> = files
        .iter()
        .map(|file| (file.relative.as_str(), file.language))
        .collect();
    let by_file: BTreeMap<&str, &FileScan> = scans
        .iter()
        .map(|scan| (scan.relative.as_str(), scan))
        .collect();
    let mut cache: BTreeMap<String, Option<ShapeMap>> = BTreeMap::new();
    for violation in violations.iter_mut() {
        let Some(&language) = languages.get(violation.file.as_str()) else {
            continue;
        };
        let Some(scan) = by_file.get(violation.file.as_str()) else {
            continue;
        };
        let base_shapes =
            cache.entry(violation.file.clone()).or_insert_with(|| {
                load_base_shapes(root, base, &violation.file, language)
            });
        if let Some(base_shapes) = base_shapes
            && ratchet_improved(violation, scan, base_shapes)
        {
            violation.improving = true;
            violation.severity = "warning".to_owned();
            violation.obligation = Some("ratchet_improving".to_owned());
        }
    }
}

type ShapeMap = BTreeMap<(String, usize), FunctionShape>;

fn shape_key(
    shapes: &[FunctionShape],
    target: &FunctionShape,
) -> (String, usize) {
    let ordinal = shapes
        .iter()
        .take_while(|shape| shape.start_line < target.start_line)
        .filter(|shape| shape.name == target.name)
        .count();
    (target.name.clone(), ordinal)
}

fn ratchet_improved(
    violation: &Violation,
    scan: &FileScan,
    base_shapes: &ShapeMap,
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
    let Some(previous) = base_shapes.get(&shape_key(&scan.shapes, current))
    else {
        return false;
    };
    match violation.message.as_str() {
        "control-flow nesting exceeds three levels" => {
            current.nesting < previous.nesting
        }
        "cyclomatic complexity exceeds ten" => {
            current.complexity < previous.complexity
        }
        "cognitive complexity exceeds fifteen (cognitive_v1 shadow)" => {
            current.cognitive < previous.cognitive
        }
        _ => false,
    }
}

fn load_base_shapes(
    root: &Path,
    base: &str,
    relative: &str,
    language: Language,
) -> Option<ShapeMap> {
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
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut indexed = BTreeMap::new();
    for shape in analyze::function_shapes(language, &source, &tree) {
        let ordinal = seen.entry(shape.name.clone()).or_default();
        indexed.insert((shape.name.clone(), *ordinal), shape);
        *ordinal += 1;
    }
    Some(indexed)
}
