use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    collect_pattern_identifiers, function_name, is_function,
    is_nesting_construct, node_text, visit,
};
use super::types::FunctionMetrics;

pub(crate) fn function_metrics(
    source: &str,
    function: Node,
) -> FunctionMetrics {
    let root_name = function_name(function).map(|name| node_text(source, name));
    let mut walk = MetricsWalk {
        root_id: function.id(),
        source,
        root_name,
        metrics: FunctionMetrics {
            nesting: 0,
            cyclomatic: 1,
            cognitive: 0,
            nesting_hotspots: Vec::new(),
            complexity_hotspots: Vec::new(),
        },
    };
    walk.step(function, 0);
    walk.metrics
}

struct MetricsWalk<'a> {
    root_id: usize,
    source: &'a str,
    root_name: Option<&'a str>,
    metrics: FunctionMetrics,
}

impl MetricsWalk<'_> {
    fn step(&mut self, node: Node, depth: usize) {
        if node.id() != self.root_id && is_function(node.kind()) {
            return;
        }
        let else_if = is_else_if(node);
        let nests = !else_if && is_nesting_construct(node.kind());
        let line = node.start_position().row + 1;
        if nests {
            self.record_nesting(node.kind(), depth + 1, line);
        }
        let (cyclomatic, cognitive) = self.node_scores(node, depth, else_if);
        self.record_complexity(node.kind(), cyclomatic, cognitive, line);
        let next_depth = if nests { depth + 1 } else { depth };
        for index in 0..node.named_child_count() {
            if let Some(child) = node.named_child(index as u32) {
                self.step(child, next_depth);
            }
        }
    }

    fn record_nesting(&mut self, kind: &str, level: usize, line: usize) {
        self.metrics.nesting = self.metrics.nesting.max(level);
        if level > 3 {
            self.metrics.nesting_hotspots.push((
                hotspot_label(kind).to_owned(),
                line,
                level,
            ));
        }
    }

    fn record_complexity(
        &mut self,
        kind: &str,
        cyclomatic: usize,
        cognitive: usize,
        line: usize,
    ) {
        self.metrics.cyclomatic += cyclomatic;
        self.metrics.cognitive += cognitive;
        if cognitive > 0 || cyclomatic > 0 {
            self.metrics.complexity_hotspots.push((
                hotspot_label(kind).to_owned(),
                line,
                cognitive.max(cyclomatic),
            ));
        }
    }

    fn node_scores(
        &self,
        node: Node,
        depth: usize,
        else_if: bool,
    ) -> (usize, usize) {
        let kind = node.kind();
        if is_structural_branch(kind) {
            return (1, if else_if { 1 } else { 1 + depth });
        }
        if kind == "elif_clause" || kind == "else_clause" {
            return (usize::from(kind == "elif_clause"), 1);
        }
        if kind == "match_arm" && node.child_by_field_name("guard").is_some() {
            return (0, 1);
        }
        if is_logical_branch(node) {
            return (1, logical_sequence_increment(node));
        }
        if kind == "goto_statement" || self.is_self_call(node) {
            return (0, 1);
        }
        (0, 0)
    }

    fn is_self_call(&self, node: Node) -> bool {
        matches!(node.kind(), "call" | "call_expression")
            && node.child_by_field_name("function").is_some_and(|callee| {
                callee.kind() == "identifier"
                    && self.root_name == Some(node_text(self.source, callee))
            })
    }
}

fn is_structural_branch(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "if_expression"
            | "if_let_expression"
            | "for_statement"
            | "for_in_statement"
            | "for_of_statement"
            | "for_expression"
            | "while_statement"
            | "while_expression"
            | "do_statement"
            | "loop_expression"
            | "catch_clause"
            | "except_clause"
            | "conditional_expression"
            | "ternary_expression"
            | "switch_statement"
            | "match_statement"
            | "match_expression"
            | "select_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
    )
}

fn is_logical_branch(node: Node) -> bool {
    if node.kind() == "boolean_operator" {
        return true;
    }
    node.kind() == "binary_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|op| matches!(op.kind(), "&&" | "||" | "and" | "or"))
}

pub(crate) fn is_else_if(node: Node) -> bool {
    matches!(
        node.kind(),
        "if_statement" | "if_expression" | "if_let_expression"
    ) && node.parent().is_some_and(|parent| {
        parent.kind() == "else_clause"
            || (parent.kind() == "if_statement"
                && parent
                    .child_by_field_name("alternative")
                    .is_some_and(|alt| alt.id() == node.id()))
    })
}

fn hotspot_label(kind: &str) -> &'static str {
    match kind {
        "if_statement" | "if_expression" | "if_let_expression" => "if",
        "elif_clause" => "elif",
        "else_clause" => "else",
        "for_statement" | "for_in_statement" | "for_of_statement"
        | "for_expression" => "for",
        "while_statement" | "while_expression" => "while",
        "do_statement" => "do",
        "loop_expression" => "loop",
        "catch_clause" | "except_clause" => "catch",
        "conditional_expression" | "ternary_expression" => "ternary",
        "switch_statement"
        | "match_statement"
        | "match_expression"
        | "select_statement"
        | "expression_switch_statement"
        | "type_switch_statement" => "switch",
        "boolean_operator" | "binary_expression" => "logic",
        "goto_statement" => "goto",
        "match_arm" => "guard",
        _ => "branch",
    }
}

pub(crate) fn top_hotspots(
    entries: &[(String, usize, usize)],
    limit: usize,
) -> Vec<String> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
    sorted
        .into_iter()
        .take(limit)
        .map(|(label, line, weight)| format!("{label}@L{line}(+{weight})"))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogicalOp {
    And,
    Or,
}

fn logical_operator_kind(node: Node) -> Option<LogicalOp> {
    match node.kind() {
        "boolean_operator" => {
            let has_or = (0..node.child_count()).any(|i| {
                node.child(i).is_some_and(|child| child.kind() == "or")
            });
            Some(if has_or {
                LogicalOp::Or
            } else {
                LogicalOp::And
            })
        }
        "binary_expression" => {
            match node.child_by_field_name("operator")?.kind() {
                "&&" | "and" => Some(LogicalOp::And),
                "||" | "or" => Some(LogicalOp::Or),
                _ => None,
            }
        }
        _ => None,
    }
}

fn logical_sequence_increment(node: Node) -> usize {
    let Some(current) = logical_operator_kind(node) else {
        return 0;
    };
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if candidate.kind() == "parenthesized_expression" {
            parent = candidate.parent();
            continue;
        }
        return match logical_operator_kind(candidate) {
            Some(parent_op) if parent_op == current => 0,
            _ => 1,
        };
    }
    1
}

pub(crate) fn collect_locals(
    language: Language,
    source: &str,
    function: Node,
) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    visit(function, &mut |node| {
        if node.id() != function.id() && is_function(node.kind()) {
            return;
        }
        if let Some(target) = local_binding_target(language, node) {
            collect_pattern_identifiers(source, target, &mut names);
        }
    });
    for ignored in ["self", "cls", "this", "_"] {
        names.remove(ignored);
    }
    names
}

fn local_binding_target(language: Language, node: Node) -> Option<Node> {
    let collects = match language {
        Language::Python => matches!(
            node.kind(),
            "assignment" | "for_statement" | "named_expression"
        ),
        Language::Rust => node.kind() == "let_declaration",
        _ if language.is_script() => node.kind() == "variable_declarator",
        _ => matches!(node.kind(), "init_declarator" | "declarator"),
    };
    if !collects {
        return None;
    }
    if language == Language::Rust {
        node.child_by_field_name("pattern")
    } else if language == Language::Python {
        node.child_by_field_name("left")
            .or_else(|| node.child_by_field_name("target"))
    } else {
        node.child_by_field_name("name")
            .or_else(|| node.child_by_field_name("declarator"))
    }
}
