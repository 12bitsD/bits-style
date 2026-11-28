use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    collect_pattern_identifiers, find_named_child_kind, has_function_ancestor,
    has_type_ancestor, node_text,
};
use super::env_checks::{
    in_python_main_guard, is_optional_import_fallback, is_type_alias,
};
use super::naming::{
    is_react_component, valid_function_name, valid_lower_snake,
    valid_pascal_case, valid_upper_snake,
};
use super::types::{Analyzer, Diag};

pub(crate) fn analyze_type(analyzer: &mut Analyzer<'_>, node: Node) {
    if let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| find_named_child_kind(node, "type_identifier"))
    {
        let text = node_text(analyzer.source, name);
        if !valid_pascal_case(text) {
            analyzer.emit(Diag::new(
                name,
                "BCS010",
                "type names must use PascalCase",
                (text, "PascalCase"),
            ));
        }
    }
    if analyzer.language == Language::Cpp {
        let text = node_text(analyzer.source, node);
        if text.contains("getInstance") && text.contains("static") {
            analyzer.emit(Diag::new(
                node,
                "BCS011",
                "singleton patterns are forbidden",
                ("singleton", "explicit_dependencies"),
            ));
        }
    }
}

pub(crate) fn analyze_variable(analyzer: &mut Analyzer<'_>, node: Node) {
    let global = !has_function_ancestor(node) && !has_type_ancestor(node);
    if global && is_mutable_global(analyzer.language, analyzer.source, node) {
        let text = node_text(analyzer.source, node);
        analyzer.emit(Diag::new(
            node,
            "BCS011",
            "mutable global state is forbidden",
            (text.lines().next().unwrap_or(text), "explicit_dependency"),
        ));
    }
    if analyzer.language == Language::Python {
        analyze_python_variable(analyzer, node, global);
    } else {
        analyze_non_python_variable(analyzer, node, global);
    }
}

fn is_mutable_global(language: Language, source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    match language {
        Language::Rust => {
            (node.kind() == "static_item" && text.contains("static mut"))
                || node.kind() == "let_declaration"
        }
        Language::Python => is_mutable_python_global(source, node),
        _ if language.is_script() => {
            matches!(
                node.kind(),
                "lexical_declaration" | "variable_declaration"
            ) && (text.trim_start().starts_with("let ")
                || text.trim_start().starts_with("var "))
        }
        _ => {
            node.kind() == "declaration"
                && !text.contains("const")
                && !text.contains("static const")
        }
    }
}

fn is_mutable_python_global(source: &str, node: Node) -> bool {
    if node.kind() != "assignment"
        || is_type_alias(Language::Python, source, node)
    {
        return false;
    }
    let name = node
        .child_by_field_name("left")
        .map_or("", |left| node_text(source, left).trim());
    let value = node
        .child_by_field_name("right")
        .map_or("", |right| node_text(source, right).trim());
    if is_exempt_python_global(source, node, name, value) {
        return false;
    }
    !valid_upper_snake(name) || is_python_mutable_literal(value)
}

fn is_exempt_python_global(
    source: &str,
    node: Node,
    name: &str,
    value: &str,
) -> bool {
    name.is_empty()
        || name.contains([',', '(', '[', '.'])
        || name.starts_with('_')
        || value.starts_with("logging.getLogger(")
        || in_python_main_guard(source, node)
        || is_optional_import_fallback(source, node, value)
}

fn is_python_mutable_literal(value: &str) -> bool {
    value.starts_with('[')
        || value.starts_with('{')
        || ["list(", "dict(", "set(", "bytearray("]
            .iter()
            .any(|prefix| value.starts_with(prefix))
}

fn analyze_python_variable(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    global: bool,
) {
    if is_type_alias(Language::Python, analyzer.source, node) {
        check_python_type_alias_name(analyzer, node);
        return;
    }
    if has_type_ancestor(node) || node.kind() != "assignment" {
        return;
    }
    let Some(target) = node.child_by_field_name("left") else {
        return;
    };
    if target.kind() == "identifier" {
        check_python_target_names(analyzer, target, global);
    }
}

fn check_python_type_alias_name(analyzer: &mut Analyzer<'_>, node: Node) {
    let Some(target) = node.child_by_field_name("left") else {
        return;
    };
    let name = node_text(analyzer.source, target);
    if !valid_pascal_case(name) {
        analyzer.emit(Diag::new(
            target,
            "BCS010",
            "type aliases must use PascalCase",
            (name, "PascalCase"),
        ));
    }
}

fn check_python_target_names(
    analyzer: &mut Analyzer<'_>,
    target: Node,
    global: bool,
) {
    let mut names = BTreeSet::new();
    collect_pattern_identifiers(analyzer.source, target, &mut names);
    for name in names {
        if (global && valid_upper_snake(&name)) || valid_lower_snake(&name) {
            continue;
        }
        analyzer.emit(Diag::new(
            target,
            "BCS010",
            "variable name does not match the language profile",
            (name, "lower_snake_case"),
        ));
    }
}

fn analyze_non_python_variable(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    global: bool,
) {
    let Some(name) = extract_binding_name(node) else {
        return;
    };
    let name_text = node_text(analyzer.source, name);
    let is_component = analyzer.language.is_script()
        && valid_pascal_case(name_text)
        && is_react_component(analyzer.language, node, name_text);
    if is_component {
        return;
    }
    if is_constant_binding(analyzer.source, node, global) {
        check_constant_name(analyzer, name, name_text);
    } else if !valid_function_name(analyzer.language, name_text) {
        let expected = if analyzer.language.is_script() {
            "lowerCamelCase"
        } else {
            "lower_snake_case"
        };
        analyzer.emit(Diag::new(
            name,
            "BCS010",
            "variable name does not match the language profile",
            (name_text, expected),
        ));
    }
}

fn extract_binding_name(node: Node) -> Option<Node> {
    let name = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("declarator"))
        .or_else(|| node.child_by_field_name("pattern"))?;
    matches!(
        name.kind(),
        "identifier" | "shorthand_property_identifier_pattern"
    )
    .then_some(name)
}

fn check_constant_name(
    analyzer: &mut Analyzer<'_>,
    name: Node,
    name_text: &str,
) {
    if !valid_upper_snake(name_text)
        && !valid_function_name(analyzer.language, name_text)
    {
        analyzer.emit(Diag::new(
            name,
            "BCS010",
            "constants must use UPPER_SNAKE_CASE",
            (name_text, "UPPER_SNAKE_CASE"),
        ));
    }
}

fn is_constant_binding(source: &str, node: Node, global: bool) -> bool {
    if matches!(node.kind(), "preproc_def" | "const_item" | "static_item") {
        return true;
    }
    global
        && (node_text(source, node).trim_start().starts_with("const ")
            || node.parent().is_some_and(|parent| {
                node_text(source, parent).trim_start().starts_with("const ")
            }))
}
