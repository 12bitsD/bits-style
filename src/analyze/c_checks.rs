use tree_sitter::Node;

use super::ast::{
    find_named_child_kind, function_name, is_function, meaningful_statements,
    node_text, visit,
};
use super::text_util::contains_word;
use super::types::{Analyzer, Diag};

pub(crate) fn has_assert_side_effect(node: Node) -> bool {
    if matches!(
        node.kind(),
        "named_expression"
            | "assignment"
            | "augmented_assignment"
            | "assignment_expression"
            | "update_expression"
    ) {
        return true;
    }
    (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32).is_some_and(|child| {
            child.kind() != "comment" && has_assert_side_effect(child)
        })
    })
}

pub(crate) fn push_assert_side_effect(analyzer: &mut Analyzer<'_>, node: Node) {
    let violation = analyzer.emit(Diag::new(
        node,
        "BCS006",
        "assertion condition must not contain explicit side effects",
        ("assert_side_effect", "separate_statement_before_assert"),
    ));
    violation.mark_high_confidence_violation();
}

pub(crate) fn analyze_assert_call_side_effects(
    analyzer: &mut Analyzer<'_>,
    node: Node,
) {
    let Some(callee) = node
        .child_by_field_name("function")
        .map(|function| node_text(analyzer.source, function))
    else {
        return;
    };
    if !matches!(callee, "assert" | "static_assert" | "_Static_assert") {
        return;
    }
    if let Some(arguments) = node.child_by_field_name("arguments")
        && has_assert_side_effect(arguments)
    {
        push_assert_side_effect(analyzer, node);
    }
}

pub(crate) fn analyze_macro(analyzer: &mut Analyzer<'_>, node: Node) {
    let text = node_text(analyzer.source, node);
    for keyword in ["return", "goto", "break", "continue"] {
        if contains_word(text, keyword) {
            analyzer.emit(Diag::new(
                node,
                "BCS012",
                "function-like macros must not hide control flow",
                (keyword, "explicit_control_flow"),
            ));
        }
    }
    let statements = text.matches(';').count();
    if statements > 1
        && !(text.contains("do")
            && text.contains("while")
            && text.contains("(0)"))
    {
        analyzer.emit(Diag::new(
            node,
            "BCS012",
            "multi-statement macros must use a safe do-while block",
            ("unwrapped_macro", "do_while_0"),
        ));
    }
}

pub(crate) fn analyze_inline(analyzer: &mut Analyzer<'_>, root: Node) {
    let mut inline_fns = Vec::new();
    visit(root, &mut |node| {
        if is_function(node.kind())
            && contains_word(node_text(analyzer.source, node), "inline")
        {
            inline_fns.push(node);
        }
    });
    for node in inline_fns {
        check_inline_function(analyzer, node);
    }
}

fn check_inline_function(analyzer: &mut Analyzer<'_>, node: Node) {
    let text = node_text(analyzer.source, node);
    let lines = node.end_position().row - node.start_position().row + 1;
    if lines > 5 {
        analyzer.emit(Diag::new(
            node,
            "BCS012",
            "explicit inline function body exceeds three lines",
            (lines.saturating_sub(2), "<=3"),
        ));
    }
    if contains_word(text, "static")
        && let Some(name) =
            function_name(node).map(|name| node_text(analyzer.source, name))
        && analyzer.source.match_indices(name).count() == 2
    {
        analyzer.emit(Diag::new(
            node,
            "BCS012",
            "single-use static functions must not be manually inlined",
            ("single_use_inline", "compiler_decides"),
        ));
    }
}

pub(crate) fn analyze_try_rethrow(analyzer: &mut Analyzer<'_>, node: Node) {
    let mut handlers = 0;
    let mut only_rethrows = true;
    for index in 0..node.named_child_count() {
        let Some(child) = node.named_child(index as u32) else {
            continue;
        };
        match child.kind() {
            "except_clause" | "catch_clause" => {
                handlers += 1;
                if !handler_only_rethrows(analyzer.source, child) {
                    only_rethrows = false;
                }
            }
            "else_clause" | "finally_clause" => only_rethrows = false,
            _ => {}
        }
    }
    if handlers == 0 || !only_rethrows {
        return;
    }
    analyzer.emit_taste(
        Diag::new(
            node,
            "BCS013",
            "useless try block that only rethrows",
            ("rethrow_only_try", "delete_try_wrapper"),
        ),
        "control-flow",
    );
}

fn handler_only_rethrows(source: &str, child: Node) -> bool {
    let Some(body) = child
        .child_by_field_name("body")
        .or_else(|| find_named_child_kind(child, "block"))
    else {
        return false;
    };
    let statements = meaningful_statements(body);
    statements.len() == 1
        && matches!(node_text(source, statements[0]).trim(), "raise" | "throw;")
}
