mod defaults;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::analyze::{self, ImportRecord};
use crate::language::Language;
use crate::model::{Position, Range as ModelRange, Violation};

pub struct IoPolicy {
    calls: GlobSet,
    modules: GlobSet,
}

impl IoPolicy {
    pub fn new(extra_sources: &[String]) -> Result<Self, String> {
        Ok(Self {
            calls: build_globs(
                defaults::DEFAULT_IO_CALLS.iter().copied(),
                extra_sources,
            )?,
            modules: build_globs(
                defaults::DEFAULT_IO_MODULES.iter().copied(),
                &[],
            )?,
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

pub struct FactInput<'a> {
    pub language: Language,
    pub relative: &'a str,
    pub source: &'a str,
    pub tree: &'a Tree,
    pub imports: &'a [ImportRecord],
    pub policy: &'a IoPolicy,
    pub file_is_pure: bool,
}

struct FactCollector<'a> {
    relative: &'a str,
    source: &'a str,
    comments: Vec<Node<'a>>,
    io_bindings: BTreeSet<String>,
    policy: &'a IoPolicy,
    file_is_pure: bool,
}

pub fn extract_facts(input: &FactInput<'_>) -> Vec<FunctionFact> {
    let _ = input.language;
    let mut comments = Vec::new();
    collect_nodes(input.tree.root_node(), "comment", &mut comments);
    let io_bindings = input
        .imports
        .iter()
        .filter(|import| input.policy.is_module(&import.specifier))
        .filter_map(|import| import.binding.clone())
        .filter(|binding| binding != "*")
        .collect();
    let collector = FactCollector {
        relative: input.relative,
        source: input.source,
        comments,
        io_bindings,
        policy: input.policy,
        file_is_pure: input.file_is_pure,
    };
    let mut facts = Vec::new();
    collector.collect_functions(input.tree.root_node(), &mut facts);
    facts
}

fn collect_nodes<'a>(node: Node<'a>, kind: &str, output: &mut Vec<Node<'a>>) {
    if node.kind() == kind
        || (kind == "comment"
            && matches!(node.kind(), "line_comment" | "block_comment"))
    {
        output.push(node);
    }
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            collect_nodes(child, kind, output);
        }
    }
}

impl<'a> FactCollector<'a> {
    fn collect_functions(&self, node: Node<'a>, facts: &mut Vec<FunctionFact>) {
        if let Some(fact) = self.build_fact(node) {
            facts.push(fact);
        }
        for index in 0..node.named_child_count() {
            if let Some(child) = node.named_child(index as u32) {
                self.collect_functions(child, facts);
            }
        }
    }

    fn build_fact(&self, node: Node<'a>) -> Option<FunctionFact> {
        if !analyze::is_function(node.kind()) {
            return None;
        }
        let name_node = analyze::function_name(node)?;
        let name = analyze::node_text(self.source, name_node).trim();
        if name.is_empty() || name.contains([' ', '(', ')', '=']) {
            return None;
        }
        let mut fact = FunctionFact {
            file: self.relative.to_owned(),
            name: name.to_owned(),
            start_line: node.start_position().row + 1,
            start_column: node.start_position().column + 1,
            end_line: node.end_position().row + 1,
            pure: self.file_is_pure || self.has_pure_spec(node),
            ..FunctionFact::default()
        };
        self.collect_calls(node, node.id(), &mut fact);
        Some(fact)
    }

    fn has_pure_spec(&self, function: Node<'_>) -> bool {
        static SPEC: OnceLock<Regex> = OnceLock::new();
        let spec =
            SPEC.get_or_init(|| Regex::new(r"bits:spec\(pure\)").unwrap());
        let start = function.start_position().row + 1;
        self.comments.iter().any(|comment| {
            let line = comment.start_position().row + 1;
            (line == start || line + 1 == start)
                && comment
                    .utf8_text(self.source.as_bytes())
                    .is_ok_and(|text| spec.is_match(text))
        })
    }

    fn collect_calls(
        &self,
        node: Node<'a>,
        root_id: usize,
        fact: &mut FunctionFact,
    ) {
        if node.id() != root_id && analyze::is_function(node.kind()) {
            return;
        }
        if matches!(node.kind(), "call" | "call_expression")
            && let Some(callee) = node
                .child_by_field_name("function")
                .map(|function| compact_text(self.source, function))
            && !callee.is_empty()
        {
            let pos = (
                node.start_position().row + 1,
                node.start_position().column + 1,
            );
            if self.policy.is_io_call(&callee, &self.io_bindings) {
                fact.io_calls.push((callee.clone(), pos.0, pos.1));
            }
            if !callee.contains(['.', ':']) {
                fact.local_calls.push((callee, pos.0, pos.1));
            }
        }
        for index in 0..node.named_child_count() {
            if let Some(child) = node.named_child(index as u32) {
                self.collect_calls(child, root_id, fact);
            }
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

pub fn check_purity(
    facts: &[FunctionFact],
    resolve_import: ImportResolver<'_>,
) -> Vec<Violation> {
    let local_index: BTreeMap<(&str, &str), usize> = facts
        .iter()
        .enumerate()
        .map(|(index, fact)| ((fact.file.as_str(), fact.name.as_str()), index))
        .collect();
    let mut tainted = seed_direct_taint(facts);
    propagate_taint(facts, &local_index, resolve_import, &mut tainted);
    facts
        .iter()
        .enumerate()
        .filter_map(|(index, fact)| {
            fact.pure
                .then(|| tainted.get(&index))
                .flatten()
                .map(|sources| purity_violation(fact, sources))
        })
        .collect()
}

fn seed_direct_taint(facts: &[FunctionFact]) -> BTreeMap<usize, Vec<String>> {
    let mut tainted = BTreeMap::new();
    for (index, fact) in facts.iter().enumerate() {
        if !fact.io_calls.is_empty() {
            let sources =
                fact.io_calls.iter().map(|(n, _, _)| n.clone()).collect();
            tainted.insert(index, sources);
        }
    }
    tainted
}

fn purity_violation(fact: &FunctionFact, raw: &[String]) -> Violation {
    let mut sources = raw.to_vec();
    sources.sort();
    sources.dedup();
    let joined = sources.join(", ");
    let mut violation = Violation::new(
        "BCS017",
        &fact.file,
        fact.start_line,
        fact.start_column,
        "pure function must not call IO-tainted code",
        joined.clone(),
        "pure_computation",
        false,
    );
    violation.range = ModelRange {
        start: Position::new(fact.start_line, fact.start_column),
        end: Position::new(fact.end_line, fact.start_column),
    };
    violation.category = Some("role".to_owned());
    violation.obligation = Some("refactor".to_owned());
    violation.prescription = Some(format!(
        "move IO calls ({joined}) to an outer boundary adapter and pass data in as explicit parameters"
    ));
    violation
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
            let inherited =
                inherited_sources(fact, local_index, resolve_import, tainted);
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

fn inherited_sources(
    fact: &FunctionFact,
    local_index: &BTreeMap<(&str, &str), usize>,
    resolve_import: ImportResolver<'_>,
    tainted: &BTreeMap<usize, Vec<String>>,
) -> Vec<String> {
    let mut inherited = BTreeSet::new();
    for (callee, _, _) in &fact.local_calls {
        let target = local_index
            .get(&(fact.file.as_str(), callee.as_str()))
            .copied()
            .or_else(|| {
                let (file, sym) = resolve_import(&fact.file, callee)?;
                local_index.get(&(file.as_str(), sym.as_str())).copied()
            });
        if let Some(sources) = target.and_then(|idx| tainted.get(&idx)) {
            inherited.extend(sources.iter().cloned());
        }
    }
    inherited.into_iter().collect()
}
