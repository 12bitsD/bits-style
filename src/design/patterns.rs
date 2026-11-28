use std::collections::BTreeSet;

use tree_sitter::Node;

use crate::analyze::{
    self, count_identifier, find_parameters, function_name,
    meaningful_statements, node_text, parameter_name, parameter_nodes,
};
use crate::design::DesignContext;
use crate::language::Language;
use crate::model::Violation;

pub(crate) struct DispatchChain {
    subject: String,
    literals: BTreeSet<String>,
    line: usize,
    column: usize,
}

pub(crate) fn extract_dispatch_chain(
    source: &str,
    node: Node,
) -> Option<DispatchChain> {
    if is_chained_else_if(node) {
        return None;
    }
    let conditions = collect_if_conditions(node);
    if conditions.len() < 4 {
        return None;
    }
    let (subject, literals) = extract_string_key_set(source, &conditions)?;
    if literals.len() < 4 {
        return None;
    }
    Some(DispatchChain {
        subject,
        literals,
        line: node.start_position().row + 1,
        column: node.start_position().column + 1,
    })
}

fn is_chained_else_if(node: Node) -> bool {
    node.parent().is_some_and(|p| {
        p.kind() == "else_clause"
            || (p.kind() == "if_statement"
                && p.child_by_field_name("alternative")
                    .is_some_and(|alt| alt.id() == node.id()))
    })
}

fn collect_if_conditions<'a>(start: Node<'a>) -> Vec<Node<'a>> {
    let mut conditions = Vec::new();
    let mut current = start;
    loop {
        if let Some(cond) = current.child_by_field_name("condition") {
            conditions.push(cond);
        }
        if push_python_elif_conditions(current, &mut conditions) {
            break;
        }
        match next_else_if(current) {
            Some(next) => current = next,
            None => break,
        }
    }
    conditions
}

fn push_python_elif_conditions<'a>(
    node: Node<'a>,
    conditions: &mut Vec<Node<'a>>,
) -> bool {
    let mut saw_elif = false;
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i as u32)
            && child.kind() == "elif_clause"
        {
            saw_elif = true;
            if let Some(cond) = child.child_by_field_name("condition") {
                conditions.push(cond);
            }
        }
    }
    saw_elif
}

fn next_else_if(node: Node<'_>) -> Option<Node<'_>> {
    let alt = node.child_by_field_name("alternative")?;
    if alt.kind() == "if_statement" {
        return Some(alt);
    }
    if alt.kind() == "else_clause" {
        let inner = meaningful_statements(alt).into_iter().next()?;
        if inner.kind() == "if_statement" {
            return Some(inner);
        }
    }
    None
}

fn extract_string_key_set(
    source: &str,
    conditions: &[Node],
) -> Option<(String, BTreeSet<String>)> {
    let mut subject: Option<String> = None;
    let mut literals = BTreeSet::new();
    for cond in conditions {
        let (lhs, lit) = extract_eq_string_literal(source, *cond)?;
        if let Some(expected) = &subject {
            if expected != &lhs {
                return None;
            }
        } else {
            subject = Some(lhs);
        }
        literals.insert(lit);
    }
    Some((subject?, literals))
}

fn extract_eq_string_literal(
    source: &str,
    node: Node,
) -> Option<(String, String)> {
    let text = node_text(source, node).trim();
    let inner = text
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(text)
        .trim();
    let (lhs, rhs) =
        inner.split_once("===").or_else(|| inner.split_once("=="))?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    let quoted = (rhs.starts_with('"') && rhs.ends_with('"'))
        || (rhs.starts_with('\'') && rhs.ends_with('\''));
    if lhs.is_empty() || lhs.contains(char::is_whitespace) || !quoted {
        return None;
    }
    let literal = rhs.trim_matches(['"', '\'']).to_owned();
    (!literal.is_empty()).then_some((lhs.to_owned(), literal))
}

pub(crate) fn analyze_repeated_dispatch(
    relative: &str,
    chains: &[DispatchChain],
    violations: &mut Vec<Violation>,
) {
    if chains.len() < 2 {
        return;
    }
    let mut reported_lines = BTreeSet::new();
    for i in 0..chains.len() {
        for j in (i + 1)..chains.len() {
            let (a, b) = (&chains[i], &chains[j]);
            let overlap: Vec<&String> =
                a.literals.intersection(&b.literals).collect();
            if overlap.len() < 3 || !reported_lines.insert(b.line) {
                continue;
            }
            let sample: Vec<&str> =
                overlap.iter().take(4).map(|s| s.as_str()).collect();
            let mut violation = Violation::new(
                "BCS016",
                relative,
                b.line,
                b.column,
                "repeated string-key if/elif dispatch duplicates routing already defined earlier",
                format!("{} vs L{} ({})", b.subject, a.line, sample.join(", ")),
                "single_dispatch_registry",
                false,
            );
            violation.mark_taste_warning("symmetry");
            violation.prescription = Some(format!(
                "consolidate repeated dispatch on [{}] (L{} and L{}) into a single handler map or polymorphism table",
                sample.join(", "),
                a.line,
                b.line
            ));
            violations.push(violation);
        }
    }
}

pub(crate) fn analyze_thin_wrappers(
    ctx: &mut DesignContext<'_>,
    root: Node,
    masked: &str,
) {
    if !eligible_wrapper_module(ctx.language, root) {
        return;
    }
    let reexports = if ctx.language == Language::Python {
        analyze::python_reexports(ctx.source)
    } else {
        BTreeSet::new()
    };
    for index in 0..root.named_child_count() {
        let Some(child) = root.named_child(index as u32) else {
            continue;
        };
        if let Some((name, callee)) =
            match_thin_wrapper(ctx, child, masked, &reexports)
        {
            emit_thin_wrapper(ctx, child, name, callee);
        }
    }
}

fn eligible_wrapper_module(language: Language, root: Node) -> bool {
    if language == Language::Python {
        return true;
    }
    if !language.is_script() {
        return false;
    }
    (0..root.named_child_count()).any(|index| {
        root.named_child(index as u32)
            .is_some_and(|child| child.kind() == "export_statement")
    })
}

fn match_thin_wrapper<'a>(
    ctx: &DesignContext<'a>,
    child: Node,
    masked: &str,
    reexports: &BTreeSet<String>,
) -> Option<(&'a str, &'a str)> {
    let expected = if ctx.language == Language::Python {
        "function_definition"
    } else {
        "function_declaration"
    };
    if child.kind() != expected {
        return None;
    }
    let name = node_text(ctx.source, function_name(child)?);
    let hidden_py = !name.starts_with('_')
        || name.starts_with("__")
        || reexports.contains(name);
    if (ctx.language == Language::Python && hidden_py)
        || count_identifier(masked, name) != 2
    {
        return None;
    }
    let params =
        parameter_nodes(ctx.language, ctx.source, find_parameters(child)?);
    let names: Vec<String> = params
        .iter()
        .filter_map(|p| parameter_name(ctx.source, *p))
        .collect();
    if names.len() < 2 {
        return None;
    }
    let call = single_returned_call(child)?;
    let arg_nodes =
        meaningful_statements(call.child_by_field_name("arguments")?);
    let same = arg_nodes.len() == names.len()
        && arg_nodes.iter().zip(names.iter()).all(|(arg, want)| {
            arg.kind() == "identifier" && node_text(ctx.source, *arg) == want
        });
    let callee =
        node_text(ctx.source, call.child_by_field_name("function")?).trim();
    same.then_some((name, callee))
}

fn single_returned_call(function: Node<'_>) -> Option<Node<'_>> {
    let stmts = meaningful_statements(function.child_by_field_name("body")?);
    if stmts.len() != 1 || stmts[0].kind() != "return_statement" {
        return None;
    }
    let ret_expr = meaningful_statements(stmts[0]).into_iter().next()?;
    let call = if matches!(ret_expr.kind(), "await" | "await_expression") {
        meaningful_statements(ret_expr).into_iter().next()?
    } else {
        ret_expr
    };
    matches!(call.kind(), "call" | "call_expression").then_some(call)
}

fn emit_thin_wrapper(
    ctx: &mut DesignContext<'_>,
    node: Node,
    name: &str,
    callee: &str,
) {
    let mut violation = Violation::new(
        "BCS003",
        ctx.relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        "single-caller thin wrapper forwards identical arguments without hiding complexity",
        format!("{name} -> {callee}"),
        "inline_or_add_real_abstraction",
        false,
    );
    violation.mark_taste_warning("level-purity");
    violation.prescription = Some(format!(
        "inline `{name}` at its sole call site and call `{callee}` directly"
    ));
    ctx.violations.push(violation);
}
