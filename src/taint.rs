use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::analyze::{self, ImportRecord};
use crate::language::Language;
use crate::model::{Position, Range as ModelRange, Violation};

const DEFAULT_IO_CALLS: &[&str] = &[
    "open",
    "print",
    "input",
    "fetch",
    "fopen",
    "fread",
    "fwrite",
    "socket",
    "connect",
    "system",
    "printf",
    "fprintf",
    "std::cout",
    "std::fs::*",
    "std::net::*",
    "std::process::*",
    "tokio::fs::*",
    "tokio::net::*",
    "tokio::process::*",
    "reqwest::*",
    "fs.*",
    "node:fs*",
    "Deno.*",
    "Bun.*",
    "*.read_text",
    "*.write_text",
    "*.read_bytes",
    "*.write_bytes",
    "fmt.Print*",
    "log.Print*",
    "log.Fatal*",
    "log.Panic*",
    "http.Get",
    "http.Head",
    "http.Post*",
    "http.ListenAndServe*",
    "http.Serve*",
];

const DEFAULT_IO_MODULES: &[&str] = &[
    "os",
    "os.*",
    "subprocess",
    "socket",
    "shutil",
    "pathlib",
    "sys",
    "httpx",
    "httpcore",
    "requests",
    "urllib",
    "urllib.*",
    "aiohttp",
    "fs",
    "node:fs",
    "node:fs/promises",
    "child_process",
    "net",
    "http",
    "https",
    "io/ioutil",
    "os/exec",
    "syscall",
];

pub struct IoPolicy {
    calls: GlobSet,
    modules: GlobSet,
}

impl IoPolicy {
    pub fn new(extra_sources: &[String]) -> Result<Self, String> {
        Ok(Self {
            calls: build_globs(DEFAULT_IO_CALLS.iter().copied(), extra_sources)?,
            modules: build_globs(DEFAULT_IO_MODULES.iter().copied(), &[])?,
        })
    }

    fn is_module(&self, specifier: &str) -> bool {
        self.modules.is_match(specifier)
    }

    fn is_io_call(&self, callee: &str, io_bindings: &BTreeSet<String>) -> bool {
        if self.calls.is_match(callee) || io_bindings.contains(callee) {
            return true;
        }
        callee
            .split(['.', ':'])
            .next()
            .is_some_and(|head| !head.is_empty() && io_bindings.contains(head))
    }
}

fn build_globs<'a>(
    defaults: impl Iterator<Item = &'a str>,
    extra: &[String],
) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for pattern in defaults {
        builder.add(Glob::new(pattern).map_err(|error| error.to_string())?);
    }
    for pattern in extra {
        builder.add(Glob::new(pattern).map_err(|error| error.to_string())?);
    }
    builder.build().map_err(|error| error.to_string())
}

#[derive(Debug, Default, Clone)]
pub struct FunctionFact {
    pub file: String,
    pub name: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub pure: bool,
    pub io_calls: Vec<(String, usize, usize)>,
    pub local_calls: Vec<(String, usize, usize)>,
}

pub fn extract_facts(
    language: Language,
    relative: &str,
    source: &str,
    tree: &Tree,
    imports: &[ImportRecord],
    policy: &IoPolicy,
) -> Vec<FunctionFact> {
    let _ = language;
    let mut comments = Vec::new();
    collect_nodes(tree.root_node(), "comment", &mut comments);
    let io_bindings = imports
        .iter()
        .filter(|import| policy.is_module(&import.specifier))
        .filter_map(|import| import.binding.clone())
        .filter(|binding| binding != "*")
        .collect::<BTreeSet<_>>();
    let mut facts = Vec::new();
    collect_function_facts(
        tree.root_node(),
        relative,
        source,
        &comments,
        &io_bindings,
        policy,
        &mut facts,
    );
    facts
}

fn collect_nodes<'a>(node: Node<'a>, kind: &str, output: &mut Vec<Node<'a>>) {
    if node.kind() == kind {
        output.push(node);
    }
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            collect_nodes(child, kind, output);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_function_facts(
    node: Node,
    relative: &str,
    source: &str,
    comments: &[Node],
    io_bindings: &BTreeSet<String>,
    policy: &IoPolicy,
    facts: &mut Vec<FunctionFact>,
) {
    if analyze::is_function(node.kind())
        && let Some(name_node) = analyze::function_name(node)
    {
        let name = analyze::node_text(source, name_node).trim();
        if !name.is_empty() && !name.contains([' ', '(', ')', '=']) {
            let mut fact = FunctionFact {
                file: relative.to_owned(),
                name: name.to_owned(),
                start_line: node.start_position().row + 1,
                start_column: node.start_position().column + 1,
                end_line: node.end_position().row + 1,
                pure: has_pure_spec(source, comments, node),
                ..FunctionFact::default()
            };
            collect_calls(node, source, io_bindings, policy, node.id(), &mut fact);
            facts.push(fact);
        }
    }
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            collect_function_facts(
                child,
                relative,
                source,
                comments,
                io_bindings,
                policy,
                facts,
            );
        }
    }
}

fn has_pure_spec(source: &str, comments: &[Node], function: Node) -> bool {
    static SPEC: OnceLock<Regex> = OnceLock::new();
    let spec = SPEC.get_or_init(|| Regex::new(r"bits:spec\(pure\)").unwrap());
    let start = function.start_position().row + 1;
    comments.iter().any(|comment| {
        let line = comment.start_position().row + 1;
        (line == start || line + 1 == start)
            && comment
                .utf8_text(source.as_bytes())
                .is_ok_and(|text| spec.is_match(text))
    })
}

fn collect_calls(
    node: Node,
    source: &str,
    io_bindings: &BTreeSet<String>,
    policy: &IoPolicy,
    root_id: usize,
    fact: &mut FunctionFact,
) {
    if node.id() != root_id && analyze::is_function(node.kind()) {
        return;
    }
    if matches!(node.kind(), "call" | "call_expression")
        && let Some(callee) = node
            .child_by_field_name("function")
            .map(|function| compact_text(source, function))
        && !callee.is_empty()
    {
        let position = (
            node.start_position().row + 1,
            node.start_position().column + 1,
        );
        if policy.is_io_call(&callee, io_bindings) {
            fact.io_calls.push((callee.clone(), position.0, position.1));
        }
        if !callee.contains(['.', ':']) {
            fact.local_calls.push((callee, position.0, position.1));
        }
    }
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            collect_calls(child, source, io_bindings, policy, root_id, fact);
        }
    }
}

fn compact_text(source: &str, node: Node) -> String {
    analyze::node_text(source, node)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

type ImportResolver<'a> = &'a dyn Fn(&str, &str) -> Option<(String, String)>;

pub fn check_purity(facts: &[FunctionFact], resolve_import: ImportResolver<'_>) -> Vec<Violation> {
    let local_index: BTreeMap<(&str, &str), usize> = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| ((fact.file.as_str(), fact.name.as_str()), index))
        .collect();
    let mut tainted: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (index, fact) in facts.iter().enumerate() {
        if !fact.io_calls.is_empty() {
            let sources = fact
                .io_calls
                .iter()
                .map(|(name, _, _)| name.clone())
                .collect();
            tainted.insert(index, sources);
        }
    }
    propagate_taint(facts, &local_index, resolve_import, &mut tainted);
    let mut violations = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        if fact.pure
            && let Some(sources) = tainted.get(&index)
        {
            let mut sources = sources.to_vec();
            sources.sort();
            sources.dedup();
            let mut violation = Violation::new(
                "BCS017",
                &fact.file,
                fact.start_line,
                fact.start_column,
                "pure function must not call IO-tainted code",
                sources.join(", "),
                "pure_computation",
                false,
            );
            violation.range = ModelRange {
                start: Position::new(fact.start_line, fact.start_column),
                end: Position::new(fact.end_line, fact.start_column),
            };
            violation.category = Some("role".to_owned());
            violation.obligation = Some("refactor".to_owned());
            violations.push(violation);
        }
    }
    violations
}

fn callee_target(
    fact: &FunctionFact,
    callee: &str,
    local_index: &BTreeMap<(&str, &str), usize>,
    resolve_import: ImportResolver<'_>,
) -> Option<usize> {
    if let Some(&target) = local_index.get(&(fact.file.as_str(), callee)) {
        return Some(target);
    }
    let (file, symbol) = resolve_import(&fact.file, callee)?;
    local_index.get(&(file.as_str(), symbol.as_str())).copied()
}

fn inherited_sources(
    fact: &FunctionFact,
    local_index: &BTreeMap<(&str, &str), usize>,
    resolve_import: ImportResolver<'_>,
    tainted: &BTreeMap<usize, Vec<String>>,
) -> Vec<String> {
    let mut inherited = BTreeSet::new();
    for (callee, _, _) in &fact.local_calls {
        let sources = callee_target(fact, callee, local_index, resolve_import)
            .and_then(|target| tainted.get(&target));
        if let Some(sources) = sources {
            inherited.extend(sources.iter().cloned());
        }
    }
    inherited.into_iter().collect()
}

fn propagate_taint(
    facts: &[FunctionFact],
    local_index: &BTreeMap<(&str, &str), usize>,
    resolve_import: ImportResolver<'_>,
    tainted: &mut BTreeMap<usize, Vec<String>>,
) {
    loop {
        let mut changed = false;
        for (index, fact) in facts.iter().enumerate() {
            if tainted.contains_key(&index) {
                continue;
            }
            let inherited = inherited_sources(fact, local_index, resolve_import, tainted);
            if !inherited.is_empty() {
                tainted.insert(index, inherited);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}
