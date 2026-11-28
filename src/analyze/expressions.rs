use tree_sitter::Node;

use super::ast::{meaningful_statements, node_text};
use super::types::{Analyzer, Diag};

pub(crate) fn is_nested_ternary(node: Node) -> bool {
    let mut parent = node.parent();
    while let Some(ancestor) = parent {
        if ancestor.kind() == "parenthesized_expression" {
            parent = ancestor.parent();
            continue;
        }
        return matches!(
            ancestor.kind(),
            "conditional_expression" | "ternary_expression"
        );
    }
    false
}

pub(crate) fn is_double_negation(source: &str, node: Node) -> bool {
    if !is_negation_node(source, node) {
        return false;
    }
    let mut inner = meaningful_statements(node).into_iter().next();
    while let Some(current) = inner {
        if current.kind() == "parenthesized_expression" {
            inner = meaningful_statements(current).into_iter().next();
            continue;
        }
        return is_negation_node(source, current);
    }
    false
}

fn is_negation_node(source: &str, node: Node) -> bool {
    let text = node_text(source, node).trim_start();
    match node.kind() {
        "not_operator" => text.starts_with("not "),
        "unary_expression" | "unary_operator" => text.starts_with('!'),
        _ => false,
    }
}

pub(crate) fn push_chained_assignment(analyzer: &mut Analyzer<'_>, node: Node) {
    let mut rightmost = node;
    while let Some(next) = rightmost.child_by_field_name("right") {
        rightmost = next;
        if !matches!(next.kind(), "assignment" | "assignment_expression") {
            break;
        }
    }
    let rhs_text = node_text(analyzer.source, rightmost).trim();
    let dangerous = is_mutable_literal_or_call(rhs_text)
        || matches!(
            rightmost.kind(),
            "call"
                | "call_expression"
                | "new_expression"
                | "array"
                | "object"
                | "list"
                | "dictionary"
                | "set"
        );
    let violation = analyzer.emit(Diag::new(
        node,
        "BCS006",
        "chained assignment is forbidden",
        ("chained_assignment", "separate_statements"),
    ));
    if !dangerous {
        violation.severity = "warning".to_owned();
        violation.confidence = "low".to_owned();
    }
}

fn is_mutable_literal_or_call(rhs: &str) -> bool {
    let trimmed = rhs.trim();
    trimmed.starts_with('[')
        || trimmed.starts_with('{')
        || [
            "list(",
            "dict(",
            "set(",
            "bytearray(",
            "defaultdict(",
            "Counter(",
            "deque(",
        ]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
}
