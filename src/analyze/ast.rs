use std::collections::BTreeSet;

use tree_sitter::{Node, Point};

use crate::language::Language;

pub(crate) fn visit<'tree>(
    node: Node<'tree>,
    callback: &mut impl FnMut(Node<'tree>),
) {
    callback(node);
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            visit(child, callback);
        }
    }
}

pub(crate) fn node_text<'a>(source: &'a str, node: Node) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

pub(crate) fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "function_expression"
            | "arrow_function"
            | "method_definition"
            | "generator_function_declaration"
            | "generator_function"
            | "function_item"
            | "closure_expression"
            | "function_signature_item"
            | "method_declaration"
            | "func_literal"
    )
}

pub(crate) fn is_nesting_construct(kind: &str) -> bool {
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
            | "switch_statement"
            | "match_statement"
            | "match_expression"
            | "select_statement"
            | "expression_switch_statement"
            | "type_switch_statement"
    )
}

pub(crate) fn is_variable_declaration(language: Language, node: Node) -> bool {
    match language {
        Language::Python => node.kind() == "assignment",
        Language::Rust => matches!(
            node.kind(),
            "let_declaration" | "const_item" | "static_item"
        ),
        _ if language.is_script() => matches!(
            node.kind(),
            "lexical_declaration"
                | "variable_declaration"
                | "variable_declarator"
        ),
        _ => matches!(
            node.kind(),
            "declaration" | "init_declarator" | "preproc_def"
        ),
    }
}

pub(crate) fn is_statement_list(kind: &str) -> bool {
    matches!(
        kind,
        "compound_statement"
            | "statement_block"
            | "block"
            | "module"
            | "program"
            | "source_file"
    )
}

pub(crate) fn is_statement(kind: &str) -> bool {
    kind != "empty_statement" && kind.ends_with("_statement")
        || matches!(
            kind,
            "expression_statement"
                | "declaration"
                | "lexical_declaration"
                | "variable_declaration"
                | "return_statement"
                | "throw_statement"
        )
}

pub(crate) fn is_block(node: Node) -> bool {
    matches!(
        node.kind(),
        "compound_statement" | "statement_block" | "block"
    )
}

pub(crate) fn meaningful_statements(node: Node) -> Vec<Node> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .filter(|child| child.kind() != "comment")
        .collect()
}

pub(crate) fn is_terminating(node: Node) -> bool {
    matches!(
        node.kind(),
        "return_statement"
            | "throw_statement"
            | "return_expression"
            | "break_expression"
            | "continue_expression"
    )
}

pub(crate) fn ends_with_terminator(source: &str, node: Node) -> bool {
    if is_terminating(node) {
        return true;
    }
    meaningful_statements(node)
        .into_iter()
        .rev()
        .find(|child| is_statement(child.kind()))
        .is_some_and(|statement| {
            is_terminating(statement)
                || matches!(
                    node_text(source, statement).split_whitespace().next(),
                    Some("return" | "throw")
                )
        })
}

pub(crate) fn contains_assignment_condition(node: Node) -> bool {
    let mut found = false;
    visit(node, &mut |child| {
        if matches!(
            child.kind(),
            "assignment_expression" | "named_expression" | "assignment"
        ) {
            found = true;
        }
    });
    found
}

pub(crate) fn has_function_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if is_function(ancestor.kind()) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

pub(crate) fn has_type_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if matches!(
            ancestor.kind(),
            "class_definition"
                | "class_declaration"
                | "class_specifier"
                | "struct_specifier"
        ) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

pub(crate) fn find_parameters(function: Node) -> Option<Node> {
    function
        .child_by_field_name("parameters")
        .or_else(|| {
            let declarator =
                find_named_child_kind(function, "function_declarator")?;
            declarator.child_by_field_name("parameters")
        })
        .or_else(|| find_named_child_kind(function, "closure_parameters"))
}

pub(crate) fn function_name(function: Node) -> Option<Node> {
    if let Some(name) = function.child_by_field_name("name") {
        return Some(name);
    }
    let declarator = find_named_child_kind(function, "function_declarator")?;
    let name = declarator.child_by_field_name("declarator")?;
    if matches!(
        name.kind(),
        "identifier"
            | "field_identifier"
            | "qualified_identifier"
            | "destructor_name"
            | "operator_name"
    ) {
        Some(name)
    } else {
        find_named_child_kind(name, "identifier")
            .or_else(|| find_named_child_kind(name, "field_identifier"))
    }
}

pub(crate) fn parameter_name(source: &str, parameter: Node) -> Option<String> {
    if matches!(parameter.kind(), "object_pattern" | "array_pattern") {
        return None;
    }
    if parameter.kind() == "identifier" {
        return Some(node_text(source, parameter).to_owned());
    }
    parameter
        .child_by_field_name("name")
        .or_else(|| parameter.child_by_field_name("declarator"))
        .and_then(|node| {
            if node.kind() == "identifier" {
                Some(node)
            } else {
                find_named_child_kind(node, "identifier")
            }
        })
        .or_else(|| find_named_child_kind(parameter, "identifier"))
        .map(|node| node_text(source, node).to_owned())
}

pub(crate) fn find_named_child_kind<'tree>(
    node: Node<'tree>,
    kind: &str,
) -> Option<Node<'tree>> {
    let mut found = None;
    visit(node, &mut |child| {
        if found.is_none() && child.kind() == kind {
            found = Some(child);
        }
    });
    found
}

pub(crate) fn first_token(
    source: &str,
    node: Node,
    token: char,
) -> Option<Point> {
    let text = node_text(source, node);
    text.find(token).map(|index| {
        let prefix = &text[..index];
        let row = node.start_position().row
            + prefix.bytes().filter(|byte| *byte == b'\n').count();
        let column = if let Some(last) = prefix.rfind('\n') {
            prefix[last + 1..].len()
        } else {
            node.start_position().column + prefix.len()
        };
        Point { row, column }
    })
}

pub(crate) fn collect_pattern_identifiers(
    source: &str,
    node: Node,
    names: &mut BTreeSet<String>,
) {
    if matches!(
        node.kind(),
        "identifier"
            | "shorthand_property_identifier"
            | "shorthand_property_identifier_pattern"
    ) {
        names.insert(node_text(source, node).to_owned());
        return;
    }
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            collect_pattern_identifiers(source, child, names);
        }
    }
}
