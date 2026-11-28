use tree_sitter::Node;

use super::ast::{is_block, meaningful_statements, node_text};
use super::metrics::is_else_if;
use super::types::{Analyzer, Diag};

pub(super) type IfBranches<'a> = Vec<(Option<Node<'a>>, Node<'a>)>;

pub(super) fn has_elif_or_else(node: Node) -> bool {
    (0..node.named_child_count()).any(|i| {
        node.named_child(i as u32)
            .is_some_and(|c| matches!(c.kind(), "elif_clause" | "else_clause"))
    })
}

pub(super) fn analyze_collapsible_if(analyzer: &mut Analyzer<'_>, node: Node) {
    if node.kind() == "if_let_expression"
        || is_else_if(node)
        || node.child_by_field_name("initializer").is_some()
        || node.child_by_field_name("alternative").is_some()
        || has_elif_or_else(node)
    {
        return;
    }
    let Some(consequence) = node.child_by_field_name("consequence") else {
        return;
    };
    let statements = meaningful_statements(consequence);
    let [inner] = statements.as_slice() else {
        return;
    };
    if inner.kind() != "if_statement"
        || inner.child_by_field_name("initializer").is_some()
        || inner.child_by_field_name("alternative").is_some()
        || has_elif_or_else(*inner)
    {
        return;
    }
    analyzer.emit_taste(
        Diag::new(
            node,
            "BCS005",
            "collapsible nested if statements",
            ("nested_if", "combined_condition"),
        ),
        "control-flow",
    );
}

pub(super) fn analyze_identical_adjacent_arms(
    analyzer: &mut Analyzer<'_>,
    branches: &[(Option<Node>, Node)],
) {
    for pair in branches.windows(2) {
        let [(Some(cond_a), body_a), (Some(cond_b), body_b)] = pair else {
            continue;
        };
        if has_type_narrowing(analyzer.source, *cond_a)
            || has_type_narrowing(analyzer.source, *cond_b)
        {
            continue;
        }
        if expression_key(analyzer.source, *body_a)
            != expression_key(analyzer.source, *body_b)
        {
            continue;
        }
        analyzer.emit_taste(
            Diag::new(
                *cond_a,
                "BCS016",
                "adjacent branches have identical bodies",
                ("identical_branch_bodies", "merged_condition"),
            ),
            "symmetry",
        );
    }
}

fn has_type_narrowing(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    text.contains("typeof ")
        || text.contains("isinstance(")
        || text.contains(":=")
        || matches!(node.kind(), "if_let_expression" | "let_condition")
}

pub(super) fn collect_if_branches(
    node: Node<'_>,
) -> Option<(IfBranches<'_>, bool)> {
    if is_else_if(node) {
        return None;
    }
    let mut branches = Vec::new();
    let mut current = node;
    let exhaustive = loop {
        let condition = current.child_by_field_name("condition");
        let consequence = current.child_by_field_name("consequence")?;
        branches.push((condition, consequence));
        if has_elif_or_else(current) {
            break collect_python_branches(current, &mut branches)?;
        }
        match step_alternative(current, &mut branches)? {
            Some(next) => current = next,
            None => break current.child_by_field_name("alternative").is_some(),
        }
    };
    Some((branches, exhaustive))
}

fn collect_python_branches<'a>(
    node: Node<'a>,
    branches: &mut IfBranches<'a>,
) -> Option<bool> {
    let mut has_else = false;
    for index in 0..node.named_child_count() {
        let Some(child) = node.named_child(index as u32) else {
            continue;
        };
        if child.kind() == "elif_clause" {
            branches.push((
                child.child_by_field_name("condition"),
                child.child_by_field_name("consequence")?,
            ));
        } else if child.kind() == "else_clause" {
            branches
                .push((None, meaningful_statements(child).into_iter().next()?));
            has_else = true;
        }
    }
    Some(has_else)
}

fn step_alternative<'a>(
    current: Node<'a>,
    branches: &mut IfBranches<'a>,
) -> Option<Option<Node<'a>>> {
    let Some(alt) = current.child_by_field_name("alternative") else {
        return Some(None);
    };
    match alt.kind() {
        "if_statement" | "if_expression" => Some(Some(alt)),
        "else_clause" => {
            let child = meaningful_statements(alt).into_iter().next()?;
            if matches!(child.kind(), "if_statement" | "if_expression") {
                Some(Some(child))
            } else {
                branches.push((None, child));
                Some(None)
            }
        }
        _ => {
            branches.push((None, alt));
            Some(None)
        }
    }
}

pub(super) fn analyze_branch_head_duplication(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    branches: &[(Option<Node>, Node)],
) {
    if branches.len() < 2 {
        return;
    }
    let Some(calls) = collect_leading_calls(analyzer.source, branches) else {
        return;
    };
    let Some((first_key, first_display)) = calls.first() else {
        return;
    };
    if !calls.iter().all(|(key, _)| key == first_key) {
        return;
    }
    let violation = analyzer.emit_ranged(Diag::new(
        node,
        "BCS016",
        "mutually exclusive branches repeat the same leading call",
        (first_display.clone(), "hoist_shared_setup_or_bind_handler"),
    ));
    violation.mark_taste_warning("symmetry");
}

fn collect_leading_calls(
    source: &str,
    branches: &[(Option<Node>, Node)],
) -> Option<Vec<(String, String)>> {
    let mut calls = Vec::new();
    for (condition, body) in branches {
        if condition.is_some_and(has_expression_side_effect) {
            return None;
        }
        calls.push(leading_pure_call(source, *body)?);
    }
    Some(calls)
}

fn leading_pure_call(source: &str, body: Node) -> Option<(String, String)> {
    let first = if is_block(body) {
        meaningful_statements(body).into_iter().next()?
    } else {
        body
    };
    let expr = unwrap_single_child(first, "expression_statement")?;
    let (is_await, call) =
        if matches!(expr.kind(), "await" | "await_expression") {
            (true, single_meaningful_child(expr)?)
        } else {
            (false, expr)
        };
    if !matches!(call.kind(), "call" | "call_expression") {
        return None;
    }
    if call
        .child_by_field_name("arguments")
        .is_some_and(has_expression_side_effect)
        || call
            .child_by_field_name("function")
            .is_some_and(has_expression_side_effect)
    {
        return None;
    }
    let key = format!("await={is_await}:{}", expression_key(source, expr));
    Some((key, node_text(source, expr).trim().to_owned()))
}

fn unwrap_single_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    if node.kind() == kind {
        single_meaningful_child(node)
    } else {
        Some(node)
    }
}

fn single_meaningful_child(node: Node) -> Option<Node> {
    let mut children = meaningful_statements(node);
    (children.len() == 1).then(|| children.remove(0))
}

fn expression_key(source: &str, node: Node) -> String {
    let count = node.named_child_count();
    if count == 0 {
        return format!("{}:{}", node.kind(), node_text(source, node).trim());
    }
    let children = (0..count)
        .filter_map(|index| node.named_child(index as u32))
        .filter(|child| child.kind() != "comment")
        .map(|child| expression_key(source, child))
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({children})", node.kind())
}

fn has_expression_side_effect(node: Node) -> bool {
    let kind = node.kind();
    let impure = kind.contains("call")
        || kind.contains("assign")
        || kind.contains("update")
        || kind.contains("await")
        || matches!(
            kind,
            "lambda"
                | "lambda_expression"
                | "closure_expression"
                | "arrow_function"
                | "yield"
                | "yield_expression"
        );
    impure
        || (0..node.named_child_count()).any(|index| {
            node.named_child(index as u32)
                .is_some_and(has_expression_side_effect)
        })
}
