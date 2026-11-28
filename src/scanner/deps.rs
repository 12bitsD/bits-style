use std::collections::{BTreeMap, BTreeSet};

use crate::analyze::{self, ImportRecord};
use crate::layers::LayerPolicy;
use crate::model::Violation;
use crate::taint::{self, FunctionFact};

use super::FileScan;
use super::policy::ScanBundle;
use super::resolve::resolve_dependency;

type DepGraph<'a> = BTreeMap<&'a str, Vec<&'a str>>;
type DepEdges<'a> = BTreeMap<(&'a str, &'a str), &'a ImportRecord>;

pub(super) fn check_dependency_cycles(bundle: &mut ScanBundle<'_>) {
    let all: Vec<&FileScan> = bundle.all_scans().collect();
    let known: BTreeSet<&str> =
        all.iter().map(|scan| scan.relative.as_str()).collect();
    let (graph, edges) = build_dependency_graph(&all, &known);
    if bundle.diff_mode {
        report_diff_cycles(bundle.scans, &graph, &edges, bundle.violations);
        return;
    }
    let mut search = CycleSearch::default();
    for scan in bundle.scans {
        search.visit(&scan.relative, &graph, &edges, bundle.violations);
    }
}

fn build_dependency_graph<'a>(
    all: &[&'a FileScan],
    known: &BTreeSet<&'a str>,
) -> (DepGraph<'a>, DepEdges<'a>) {
    let mut graph: DepGraph<'a> = BTreeMap::new();
    let mut edges: DepEdges<'a> = BTreeMap::new();
    for scan in all {
        add_scan_edges(scan, known, &mut graph, &mut edges);
    }
    (graph, edges)
}

fn add_scan_edges<'a>(
    scan: &'a FileScan,
    known: &BTreeSet<&'a str>,
    graph: &mut DepGraph<'a>,
    edges: &mut DepEdges<'a>,
) {
    for import in &scan.imports {
        if import.type_only {
            continue;
        }
        let Some(specifier) = analyze::dependency_path(import) else {
            continue;
        };
        let Some(target) = resolve_dependency(
            &scan.relative,
            &specifier,
            scan.python_package.as_ref(),
            known,
        ) else {
            continue;
        };
        if target == scan.relative {
            continue;
        }
        graph.entry(&scan.relative).or_default().push(target);
        edges.insert((scan.relative.as_str(), target), import);
    }
}

fn report_diff_cycles<'a>(
    scans: &[FileScan],
    graph: &DepGraph<'a>,
    edges: &DepEdges<'a>,
    violations: &mut Vec<Violation>,
) {
    let changed: BTreeSet<&str> =
        scans.iter().map(|scan| scan.relative.as_str()).collect();
    for ((from, to), import) in edges {
        if changed.contains(*from) && dependency_reaches(graph, to, from) {
            violations.push(cycle_violation(from, to, import));
        }
    }
}

fn dependency_reaches<'a>(
    graph: &DepGraph<'a>,
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

#[derive(Default)]
struct CycleSearch<'a> {
    state: BTreeMap<&'a str, usize>,
    stack: Vec<&'a str>,
    emitted: BTreeSet<String>,
}

impl<'a> CycleSearch<'a> {
    fn visit(
        &mut self,
        file: &'a str,
        graph: &DepGraph<'a>,
        edges: &DepEdges<'a>,
        violations: &mut Vec<Violation>,
    ) {
        if self.state.get(file) == Some(&2) {
            return;
        }
        if self.state.get(file) == Some(&1) {
            self.record_cycle(file, edges, violations);
            return;
        }
        self.state.insert(file, 1);
        self.stack.push(file);
        if let Some(targets) = graph.get(file) {
            for target in targets {
                self.visit(target, graph, edges, violations);
            }
        }
        self.stack.pop();
        self.state.insert(file, 2);
    }

    fn record_cycle(
        &mut self,
        file: &'a str,
        edges: &DepEdges<'a>,
        violations: &mut Vec<Violation>,
    ) {
        let Some(start) = self.stack.iter().position(|entry| *entry == file)
        else {
            return;
        };
        let mut cycle = self.stack[start..].to_vec();
        cycle.sort();
        if !self.emitted.insert(cycle.join("->")) {
            return;
        }
        let from = self.stack.last().copied().unwrap_or(file);
        if let Some(import) = edges.get(&(from, file)) {
            violations.push(cycle_violation(from, file, import));
        }
    }
}

fn cycle_violation(from: &str, to: &str, import: &ImportRecord) -> Violation {
    Violation::new(
        "BCS014",
        from,
        import.line,
        import.column,
        "dependency cycle detected",
        format!("{from} -> {to}"),
        "acyclic_dependencies",
        false,
    )
}

pub(super) fn check_layer_boundaries(
    bundle: &mut ScanBundle<'_>,
    layers: &LayerPolicy,
) {
    if layers.is_empty() {
        return;
    }
    let known: BTreeSet<&str> = bundle
        .all_scans()
        .map(|scan| scan.relative.as_str())
        .collect();
    for scan in bundle.scans {
        layers.check_imports(
            &scan.relative,
            &scan.imports,
            |import| {
                let specifier = analyze::dependency_path(import)?;
                resolve_dependency(
                    &scan.relative,
                    &specifier,
                    scan.python_package.as_ref(),
                    &known,
                )
                .map(str::to_owned)
            },
            bundle.violations,
        );
    }
}

pub(super) fn check_purity_contracts(bundle: &mut ScanBundle<'_>) {
    let all: Vec<&FileScan> = bundle.all_scans().collect();
    let known: BTreeSet<&str> =
        all.iter().map(|scan| scan.relative.as_str()).collect();
    let by_file: BTreeMap<&str, &FileScan> = all
        .iter()
        .map(|scan| (scan.relative.as_str(), *scan))
        .collect();
    let resolve_import =
        |file: &str, symbol: &str| -> Option<(String, String)> {
            resolve_imported_symbol(&by_file, &known, file, symbol)
        };
    let facts: Vec<FunctionFact> = all
        .iter()
        .flat_map(|scan| scan.facts.iter().cloned())
        .collect();
    bundle
        .violations
        .extend(taint::check_purity(&facts, &resolve_import));
}

fn resolve_imported_symbol(
    by_file: &BTreeMap<&str, &FileScan>,
    known: &BTreeSet<&str>,
    file: &str,
    symbol: &str,
) -> Option<(String, String)> {
    let scan = by_file.get(file)?;
    for import in &scan.imports {
        if import.binding.as_deref() != Some(symbol) {
            continue;
        }
        let specifier = analyze::dependency_path(import)?;
        let target = resolve_dependency(
            file,
            &specifier,
            scan.python_package.as_ref(),
            known,
        )?;
        return Some((target.to_owned(), symbol.to_owned()));
    }
    None
}
