use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::Node;

use crate::language::Language;

use super::ast::{node_text, parameter_name, visit};

pub(crate) fn parameter_nodes<'tree>(
    language: Language,
    source: &str,
    parameters: Node<'tree>,
) -> Vec<Node<'tree>> {
    let mut nodes: Vec<Node<'tree>> = (0..parameters.named_child_count())
        .filter_map(|index| parameters.named_child(index as u32))
        .filter(|node| is_param_child(language, source, *node))
        .collect();
    strip_receiver_param(language, source, &mut nodes);
    nodes
}

fn is_param_child(language: Language, source: &str, node: Node) -> bool {
    if node.kind() == "comment" {
        return false;
    }
    match language {
        Language::Rust => !matches!(node.kind(), "self_parameter" | "self"),
        Language::Python => is_python_param_node(source, node),
        _ => is_generic_param_node(node),
    }
}

fn is_python_param_node(source: &str, node: Node) -> bool {
    if node.kind() == "list_splat_pattern"
        && node_text(source, node).trim() == "*"
    {
        return false;
    }
    if node.kind() == "identifier"
        && node
            .parent()
            .is_some_and(|parent| parent.kind() != "parameters")
    {
        return false;
    }
    matches!(
        node.kind(),
        "identifier"
            | "default_parameter"
            | "typed_parameter"
            | "typed_default_parameter"
            | "required_parameter"
            | "optional_parameter"
            | "list_splat_pattern"
            | "dictionary_splat_pattern"
    )
}

fn is_generic_param_node(node: Node) -> bool {
    node.kind().contains("parameter")
        || matches!(
            node.kind(),
            "identifier"
                | "assignment_pattern"
                | "rest_pattern"
                | "object_pattern"
                | "array_pattern"
                | "required_parameter"
                | "optional_parameter"
        )
}

fn strip_receiver_param(
    language: Language,
    source: &str,
    nodes: &mut Vec<Node<'_>>,
) {
    let Some(first) = nodes.first().copied() else {
        return;
    };
    let Some(name) = parameter_name(source, first) else {
        return;
    };
    let is_receiver = (language == Language::Python
        && matches!(name.as_str(), "self" | "cls"))
        || (language.is_script() && name == "this");
    if is_receiver {
        nodes.remove(0);
    }
}

pub(crate) fn is_python_keyword_only(source: &str, parameter: Node) -> bool {
    let mut sibling = parameter.prev_sibling();
    while let Some(current) = sibling {
        if current.kind() == "keyword_separator"
            || node_text(source, current).trim() == "*"
        {
            return true;
        }
        sibling = current.prev_sibling();
    }
    false
}

pub(crate) fn is_framework_method_name(language: Language, name: &str) -> bool {
    language == Language::Python
        && matches!(
            name,
            "do_GET"
                | "do_POST"
                | "do_PUT"
                | "do_DELETE"
                | "do_PATCH"
                | "do_HEAD"
                | "do_OPTIONS"
        )
}

pub(crate) fn is_react_component(
    language: Language,
    function: Node,
    name: &str,
) -> bool {
    if !language.is_script() || !valid_pascal_case(name) {
        return false;
    }
    let mut has_jsx = false;
    visit(function, &mut |node| {
        if node.kind().starts_with("jsx_") {
            has_jsx = true;
        }
    });
    has_jsx
}

pub(crate) fn is_constructor_name(
    function: Node,
    source: &str,
    name: &str,
) -> bool {
    if function.kind() == "method_definition" && name == "constructor" {
        return true;
    }
    let mut current = function.parent();
    while let Some(ancestor) = current {
        if matches!(ancestor.kind(), "class_specifier" | "struct_specifier") {
            return ancestor
                .child_by_field_name("name")
                .is_some_and(|type_name| node_text(source, type_name) == name);
        }
        current = ancestor.parent();
    }
    false
}

pub(crate) fn valid_function_name(language: Language, name: &str) -> bool {
    if language.is_script() {
        valid_lower_camel(name)
    } else if language == Language::Go {
        valid_go_mixed_case(name)
    } else {
        valid_lower_snake(name)
    }
}

fn valid_go_mixed_case(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    if name == "_"
        || name.starts_with("Test")
        || name.starts_with("Benchmark")
        || name.starts_with("Example")
        || name.starts_with("Fuzz")
    {
        return true;
    }
    REGEX
        .get_or_init(|| Regex::new(r"^[A-Za-z][A-Za-z0-9]*$").unwrap())
        .is_match(name)
}

pub(crate) fn valid_lower_camel(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^[a-z][A-Za-z0-9]*$").unwrap())
        .is_match(name)
}

pub(crate) fn valid_lower_snake(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^[a-z_][a-z0-9_]*$").unwrap())
        .is_match(name)
}

pub(crate) fn valid_upper_snake(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^[A-Z_][A-Z0-9_]*$").unwrap())
        .is_match(name)
}

pub(crate) fn valid_pascal_case(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^_*[A-Z][A-Za-z0-9]*$").unwrap())
        .is_match(name)
}
