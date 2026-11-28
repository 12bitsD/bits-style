use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    has_function_ancestor, is_function, is_statement_list,
    is_variable_declaration, node_text,
};
use super::c_checks::{
    analyze_assert_call_side_effects, analyze_macro, analyze_try_rethrow,
    has_assert_side_effect, push_assert_side_effect,
};
use super::control_flow::{
    analyze_goto, analyze_if, analyze_loop, analyze_switch, analyze_unreachable,
};
use super::env_checks::analyze_hidden_input;
use super::errors::analyze_catch;
use super::expressions::{
    is_double_negation, is_nested_ternary, push_chained_assignment,
};
use super::functions::analyze_function;
use super::go_checks::analyze_go_context_field;
use super::imports::analyze_import;
use super::spacing::analyze_binary_operator;
use super::types::{Analyzer, Diag};
use super::variables::{analyze_type, analyze_variable};

pub(super) fn dispatch_node(
    analyzer: &mut Analyzer<'_>,
    root: Node,
    node: Node,
) {
    if is_function(node.kind()) {
        analyze_function(analyzer, node);
    }
    dispatch_statement_kind(analyzer, root, node);
    dispatch_expression_kind(analyzer, node);
    if is_variable_declaration(analyzer.language, node) {
        analyze_variable(analyzer, node);
    }
    if is_statement_list(node.kind()) {
        analyze_unreachable(analyzer, node);
    }
}

fn dispatch_statement_kind(
    analyzer: &mut Analyzer<'_>,
    root: Node,
    node: Node,
) {
    match node.kind() {
        "if_statement" | "if_expression" | "if_let_expression" => {
            analyze_if(analyzer, node);
        }
        "for_statement" | "while_statement" | "do_statement"
        | "for_in_statement" | "for_of_statement" | "loop_expression"
        | "while_expression" | "for_expression" => {
            analyze_loop(analyzer, node);
        }
        "switch_statement" | "match_expression" => {
            analyze_switch(analyzer, node);
        }
        "goto_statement" => analyze_goto(analyzer, root, node),
        "catch_clause" | "except_clause" => analyze_catch(analyzer, node),
        "try_statement" => analyze_try_rethrow(analyzer, node),
        "preproc_function_def" => analyze_macro(analyzer, node),
        "preproc_if" | "preproc_ifdef" | "preproc_ifndef" => {
            check_preproc_inside_function(analyzer, node);
        }
        "import_statement"
        | "import_from_statement"
        | "import_declaration"
        | "preproc_include"
        | "use_declaration" => {
            analyze_import(analyzer, node, true);
        }
        "class_definition"
        | "class_declaration"
        | "class_specifier"
        | "struct_specifier"
        | "interface_declaration"
        | "enum_specifier"
        | "enum_declaration"
        | "struct_item"
        | "enum_item"
        | "trait_item"
        | "type_item" => analyze_type(analyzer, node),
        "nonlocal_statement" | "global_statement"
            if analyzer.language == Language::Python =>
        {
            emit_python_scope_mutation(analyzer, node);
        }
        _ => {}
    }
}

fn dispatch_expression_kind(analyzer: &mut Analyzer<'_>, node: Node) {
    match node.kind() {
        "call_expression" | "call" => {
            analyze_hidden_input(analyzer, node);
            analyze_assert_call_side_effects(analyzer, node);
        }
        "field_declaration" if analyzer.language == Language::Go => {
            analyze_go_context_field(analyzer, node);
        }
        "assert_statement" if has_assert_side_effect(node) => {
            push_assert_side_effect(analyzer, node);
        }
        "binary_expression" => analyze_binary_operator(analyzer, node),
        "assignment_expression"
            if has_chained_right(node, "assignment_expression") =>
        {
            push_chained_assignment(analyzer, node);
        }
        "assignment"
            if analyzer.language == Language::Python
                && has_chained_right(node, "assignment") =>
        {
            push_chained_assignment(analyzer, node);
        }
        _ => dispatch_forbidden_expr(analyzer, node),
    }
}

fn dispatch_forbidden_expr(analyzer: &mut Analyzer<'_>, node: Node) {
    match node.kind() {
        "sequence_expression" | "comma_expression" => {
            analyzer.emit(Diag::new(
                node,
                "BCS006",
                "comma expressions must not replace braces",
                ("comma_expression", "explicit_statements"),
            ));
        }
        "conditional_expression" | "ternary_expression"
            if is_nested_ternary(node) =>
        {
            analyzer.emit(Diag::new(
                node,
                "BCS006",
                "nested ternary expressions are forbidden",
                ("nested_ternary", "explicit_branches"),
            ));
        }
        "unary_expression" | "not_operator" | "unary_operator"
            if is_double_negation(analyzer.source, node) =>
        {
            analyzer.emit(Diag::new(
                node,
                "BCS006",
                "double-negation conditions are forbidden",
                ("double_negation", "positive_condition"),
            ));
        }
        _ => {}
    }
}

fn has_chained_right(node: Node, expected_kind: &str) -> bool {
    node.child_by_field_name("right")
        .is_some_and(|right| right.kind() == expected_kind)
}

fn check_preproc_inside_function(analyzer: &mut Analyzer<'_>, node: Node) {
    if has_function_ancestor(node) {
        analyzer.emit(Diag::new(
            node,
            "BCS012",
            "conditional compilation must wrap complete declarations or \
             definitions",
            ("inside_function", "complete_declaration"),
        ));
    }
}

fn emit_python_scope_mutation(analyzer: &mut Analyzer<'_>, node: Node) {
    let is_nonlocal = node.kind() == "nonlocal_statement";
    let text = node_text(analyzer.source, node).trim();
    let violation = analyzer.emit_taste(
        Diag::new(
            node,
            "BCS011",
            "implicit cross-scope state mutation via global/nonlocal is \
             forbidden",
            (text, "explicit_state_object_or_return"),
        ),
        "state-gain",
    );
    violation.prescription = Some(
        "extract a small stateful class/struct with explicit methods or return \
         updated values instead of mutating outer bindings"
            .to_owned(),
    );
    if is_nonlocal {
        violation.mark_high_confidence_violation();
        violation.category = Some("state-gain".to_owned());
    }
}
