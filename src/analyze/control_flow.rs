use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    contains_assignment_condition, ends_with_terminator, find_parameters,
    first_token, is_block, is_statement, is_terminating, meaningful_statements,
    node_text, visit,
};
use super::branches::{
    analyze_branch_head_duplication, analyze_collapsible_if,
    analyze_identical_adjacent_arms, collect_if_branches,
};
use super::text_util::contains_word;
use super::types::{Analyzer, Diag};

pub(crate) fn analyze_if(analyzer: &mut Analyzer<'_>, node: Node) {
    check_redundant_else(analyzer, node);
    check_if_braces(analyzer, node);
    check_condition_assignment(analyzer, node);
    analyze_collapsible_if(analyzer, node);
    if let Some((branches, exhaustive)) = collect_if_branches(node) {
        analyze_identical_adjacent_arms(analyzer, &branches);
        if exhaustive {
            analyze_branch_head_duplication(analyzer, node, &branches);
        }
    }
}

fn check_redundant_else(analyzer: &mut Analyzer<'_>, node: Node) {
    if let (Some(then), Some(otherwise)) = (
        node.child_by_field_name("consequence"),
        node.child_by_field_name("alternative"),
    ) && ends_with_terminator(analyzer.source, then)
        && !otherwise.kind().contains("if")
    {
        analyzer.emit(Diag::new(
            otherwise,
            "BCS005",
            "else after a terminating branch is forbidden",
            ("redundant_else", "early_return"),
        ));
    }
}

fn check_if_braces(analyzer: &mut Analyzer<'_>, node: Node) {
    if matches!(
        analyzer.language,
        Language::Python | Language::Rust | Language::Go
    ) {
        return;
    }
    let Some(then) = node.child_by_field_name("consequence") else {
        return;
    };
    check_branch_brace(analyzer, node, then);
    if let Some(otherwise) = node.child_by_field_name("alternative") {
        let else_body = unwrap_else_body(otherwise);
        if else_body.kind() != "if_statement" {
            check_branch_brace(analyzer, otherwise, else_body);
        }
    }
}

fn unwrap_else_body(otherwise: Node) -> Node {
    if otherwise.kind() != "else_clause" {
        return otherwise;
    }
    meaningful_statements(otherwise)
        .into_iter()
        .find(|child| is_block(*child) || child.kind() == "if_statement")
        .unwrap_or(otherwise)
}

fn check_branch_brace(analyzer: &mut Analyzer<'_>, parent: Node, body: Node) {
    if is_block(body) {
        analyze_opening_brace(analyzer, parent, body, false);
    } else {
        analyzer.emit(Diag::new(
            body,
            "BCS007",
            "control-flow branches must use braces",
            ("unbraced_branch", "braced_branch"),
        ));
    }
}

fn check_condition_assignment(analyzer: &mut Analyzer<'_>, node: Node) {
    if let Some(condition) = node.child_by_field_name("condition")
        && contains_assignment_condition(condition)
    {
        analyzer.emit(Diag::new(
            condition,
            "BCS006",
            "assignment inside a condition is forbidden",
            ("condition_assignment", "separate_statement"),
        ));
    }
}

pub(crate) fn analyze_loop(analyzer: &mut Analyzer<'_>, node: Node) {
    if !matches!(
        analyzer.language,
        Language::Python | Language::Rust | Language::Go
    ) && let Some(body) = node.child_by_field_name("body")
    {
        if is_block(body) {
            analyze_opening_brace(analyzer, node, body, false);
        } else {
            analyzer.emit(Diag::new(
                body,
                "BCS007",
                "control-flow loops must use braces",
                ("unbraced_loop", "braced_loop"),
            ));
        }
    }
    check_condition_assignment(analyzer, node);
}

pub(crate) fn analyze_opening_brace(
    analyzer: &mut Analyzer<'_>,
    parent: Node,
    body: Node,
    function: bool,
) {
    let Some(brace) = first_token(analyzer.source, body, '{') else {
        return;
    };
    let header_end = if function {
        find_parameters(parent)
            .map(|p| p.end_position().row)
            .unwrap_or(parent.start_position().row)
    } else {
        parent
            .child_by_field_name("condition")
            .map(|c| c.end_position().row)
            .unwrap_or(parent.start_position().row)
    };
    let next_line_required = function && analyzer.language.is_c_like();
    if next_line_required && brace.row != header_end + 1 {
        analyzer.emit(Diag::new(
            body,
            "BCS007",
            "function opening brace must start on the next line",
            ("same_line", "next_line"),
        ));
    } else if !next_line_required && brace.row != header_end {
        analyzer.emit(Diag::new(
            body,
            "BCS007",
            "opening brace must share the statement line",
            ("next_line", "same_line"),
        ));
    }
}

pub(crate) fn analyze_switch(analyzer: &mut Analyzer<'_>, node: Node) {
    if !matches!(analyzer.language, Language::Rust | Language::Go)
        && let Some(body) = node.child_by_field_name("body")
    {
        analyze_opening_brace(analyzer, node, body, false);
    }
    let switch_column = node.start_position().column;
    let case_column = if analyzer.language.is_script() {
        switch_column + 2
    } else {
        switch_column
    };
    let mut cases = Vec::new();
    visit(node, &mut |child| {
        if matches!(
            child.kind(),
            "case_statement"
                | "switch_case"
                | "default_statement"
                | "match_arm"
        ) {
            cases.push(child);
        }
    });
    for child in cases {
        check_switch_case(analyzer, child, case_column);
    }
}

fn check_switch_case(
    analyzer: &mut Analyzer<'_>,
    child: Node,
    case_column: usize,
) {
    if !analyzer.language.is_rust()
        && child.start_position().column != case_column
    {
        analyzer.emit(Diag::new(
            child,
            "BCS008",
            "case/default indentation does not match the language profile",
            (child.start_position().column, case_column),
        ));
    }
    if analyzer.language != Language::Rust
        && case_can_fallthrough(analyzer.source, child)
        && !has_fallthrough_marker(analyzer.source, child)
    {
        analyzer.emit(Diag::new(
            child,
            "BCS008",
            "intentional fallthrough must be explicitly marked",
            ("unmarked_fallthrough", "fallthrough_marker"),
        ));
    }
}

fn case_can_fallthrough(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    let has_body = text.lines().count() > 1;
    let terminates = ["break", "return", "continue", "goto", "throw"]
        .iter()
        .any(|word| contains_word(text, word));
    has_body && !terminates
}

fn has_fallthrough_marker(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    text.contains("fallthrough") || text.contains("fall through")
}

pub(crate) fn analyze_goto(
    analyzer: &mut Analyzer<'_>,
    root: Node,
    node: Node,
) {
    if analyzer.language == Language::Cpp {
        analyzer.emit(Diag::new(
            node,
            "BCS005",
            "goto is forbidden in RAII-based languages",
            ("goto", "raii_cleanup"),
        ));
        return;
    }
    let label = node_text(analyzer.source, node)
        .trim()
        .trim_start_matches("goto")
        .trim_end_matches(';')
        .trim()
        .to_owned();
    if !(label.starts_with("out_") || label.starts_with("err_")) {
        analyzer.emit(Diag::new(
            node,
            "BCS005",
            "goto labels must use out_* or err_*",
            (label.clone(), "out_*|err_*"),
        ));
    }
    if goto_jumps_backward(analyzer.source, root, node, &label) {
        analyzer.emit(Diag::new(
            node,
            "BCS005",
            "goto must jump forward",
            ("backward_jump", "forward_jump"),
        ));
    }
}

fn goto_jumps_backward(
    source: &str,
    root: Node,
    node: Node,
    label: &str,
) -> bool {
    let prefix = format!("{label}:");
    let mut target = None;
    visit(root, &mut |candidate| {
        if candidate.kind() == "labeled_statement"
            && node_text(source, candidate)
                .trim_start()
                .starts_with(&prefix)
        {
            target = Some(candidate);
        }
    });
    target.is_some_and(|t| t.start_byte() <= node.start_byte())
}

pub(crate) fn analyze_unreachable(analyzer: &mut Analyzer<'_>, node: Node) {
    let statements = meaningful_statements(node);
    for pair in statements.windows(2) {
        if is_statement(pair[0].kind())
            && is_statement(pair[1].kind())
            && pair[0].start_position().row == pair[1].start_position().row
        {
            analyzer.emit(Diag::new(
                pair[1],
                "BCS006",
                "each statement must be on its own line",
                ("multiple_statements_per_line", "one_statement_per_line"),
            ));
        }
        if is_terminating(pair[0]) && is_statement(pair[1].kind()) {
            let line = node_text(analyzer.source, pair[1])
                .lines()
                .next()
                .unwrap_or("");
            analyzer.emit(Diag::new(
                pair[1],
                "BCS014",
                "unreachable code is forbidden",
                (line, "reachable_code"),
            ));
        }
    }
}
