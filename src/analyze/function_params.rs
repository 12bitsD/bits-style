use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    is_function, meaningful_statements, node_text, parameter_name, visit,
};
use super::go_checks::go_parameter_count;
use super::naming::is_python_keyword_only;
use super::types::{Analyzer, Diag};

pub(crate) fn analyze_parameter_mutability(
    analyzer: &mut Analyzer<'_>,
    function: Node,
    parameters: &BTreeSet<String>,
) {
    if parameters.is_empty() {
        return;
    }
    let mut hits = Vec::new();
    visit(function, &mut |node| {
        if let Some(target) =
            reassigned_param(analyzer.source, node, parameters)
        {
            hits.push((target, false));
        }
        if is_shadowing_param(
            analyzer.language,
            analyzer.source,
            node,
            parameters,
        ) {
            hits.push((node, true));
        }
    });
    for (node, is_shadow) in hits {
        if is_shadow {
            if let Some(name) = parameter_name(analyzer.source, node) {
                analyzer.emit(Diag::new(
                    node,
                    "BCS011",
                    "local variables must not shadow parameters",
                    (name, "distinct_name"),
                ));
            }
        } else {
            let name = node_text(analyzer.source, node);
            analyzer.emit(Diag::new(
                node,
                "BCS011",
                "function parameters must not be reassigned",
                (name, "local_variable"),
            ));
        }
    }
}

fn reassigned_param<'a>(
    source: &str,
    node: Node<'a>,
    parameters: &BTreeSet<String>,
) -> Option<Node<'a>> {
    if !matches!(
        node.kind(),
        "assignment" | "assignment_expression" | "augmented_assignment"
    ) {
        return None;
    }
    let target = node
        .child_by_field_name("left")
        .or_else(|| node.child_by_field_name("target"))?;
    (target.kind() == "identifier"
        && parameters.contains(node_text(source, target)))
    .then_some(target)
}

fn is_shadowing_param(
    language: Language,
    source: &str,
    node: Node,
    parameters: &BTreeSet<String>,
) -> bool {
    let declaration = match language {
        Language::C | Language::Cpp => {
            matches!(node.kind(), "init_declarator" | "declaration")
        }
        Language::Python => false,
        _ => node.kind() == "variable_declarator",
    };
    declaration
        && parameter_name(source, node)
            .is_some_and(|name| parameters.contains(&name))
}

pub(crate) fn analyze_multi_out_params(
    analyzer: &mut Analyzer<'_>,
    function: Node,
    parameters: &BTreeSet<String>,
) {
    if parameters.len() < 2
        || !(analyzer.language == Language::Python
            || analyzer.language.is_script())
    {
        return;
    }
    let (returns_value, mutated) =
        collect_mutated_params(analyzer.source, function, parameters);
    if returns_value || mutated.len() < 2 {
        return;
    }
    let mut names: Vec<String> = mutated.into_iter().collect();
    names.sort();
    let joined = names.join(", ");
    let violation = analyzer.emit_ranged(Diag::new(
        function,
        "BCS004",
        "function mutates multiple output container parameters instead of \
         returning a value",
        (joined.clone(), "return_structured_result"),
    ));
    violation.mark_taste_warning("state-gain");
    violation.prescription = Some(format!(
        "return a frozen dataclass/struct containing ({joined}) instead of \
         mutating caller containers"
    ));
}

fn collect_mutated_params(
    source: &str,
    function: Node,
    parameters: &BTreeSet<String>,
) -> (bool, BTreeSet<String>) {
    let mut returns_value = false;
    let mut mutated = BTreeSet::new();
    visit(function, &mut |node| {
        if node.id() != function.id() && is_function(node.kind()) {
            return;
        }
        if node.kind() == "return_statement" {
            if return_has_value(source, node) {
                returns_value = true;
            }
            return;
        }
        if let Some(name) = mutated_method_receiver(source, node, parameters)
            .or_else(|| mutated_subscript_receiver(source, node, parameters))
        {
            mutated.insert(name);
        }
    });
    (returns_value, mutated)
}

fn return_has_value(source: &str, node: Node) -> bool {
    meaningful_statements(node).into_iter().any(|child| {
        !matches!(node_text(source, child).trim(), "None" | "undefined")
    })
}

fn mutated_method_receiver(
    source: &str,
    node: Node,
    parameters: &BTreeSet<String>,
) -> Option<String> {
    if !matches!(node.kind(), "call" | "call_expression") {
        return None;
    }
    let callee = node.child_by_field_name("function")?;
    if !matches!(callee.kind(), "attribute" | "member_expression") {
        return None;
    }
    let obj = callee.child_by_field_name("object")?;
    let attr = callee
        .child_by_field_name("attribute")
        .or_else(|| callee.child_by_field_name("property"))?;
    if obj.kind() != "identifier" {
        return None;
    }
    let obj_name = node_text(source, obj);
    let method = node_text(source, attr);
    let is_mutator = matches!(
        method,
        "append"
            | "extend"
            | "insert"
            | "add"
            | "update"
            | "setdefault"
            | "pop"
            | "remove"
            | "clear"
            | "push"
            | "unshift"
            | "splice"
            | "set"
            | "delete"
    );
    (is_mutator && parameters.contains(obj_name)).then(|| obj_name.to_owned())
}

fn mutated_subscript_receiver(
    source: &str,
    node: Node,
    parameters: &BTreeSet<String>,
) -> Option<String> {
    if !matches!(
        node.kind(),
        "assignment" | "assignment_expression" | "augmented_assignment"
    ) {
        return None;
    }
    let target = node
        .child_by_field_name("left")
        .or_else(|| node.child_by_field_name("target"))?;
    if !matches!(target.kind(), "subscript" | "subscript_expression") {
        return None;
    }
    let obj = target
        .child_by_field_name("value")
        .or_else(|| target.child_by_field_name("object"))?;
    if obj.kind() != "identifier" {
        return None;
    }
    let obj_name = node_text(source, obj);
    parameters.contains(obj_name).then(|| obj_name.to_owned())
}

pub(crate) fn effective_param_budget(
    language: Language,
    source: &str,
    fn_name: Option<&str>,
    params: &[Node],
) -> (usize, usize) {
    if language == Language::Go {
        return (go_parameter_count(params), 4);
    }
    let total = params.len();
    if language != Language::Python {
        return (total, 4);
    }
    let positional = params
        .iter()
        .filter(|param| !is_python_keyword_only(source, **param))
        .count();
    let is_di_ctor = matches!(fn_name, Some("__init__" | "__new__"));
    if is_di_ctor && positional <= 4 {
        (total, 8)
    } else if positional > 4 {
        (positional, 4)
    } else if positional < total {
        (total, 6)
    } else {
        (total, 4)
    }
}
