use tree_sitter::Node;

use super::ast::{find_named_child_kind, meaningful_statements, node_text};
use super::types::{Analyzer, Diag};

pub(crate) fn analyze_catch(analyzer: &mut Analyzer<'_>, node: Node) {
    let Some(body) = node
        .child_by_field_name("body")
        .or_else(|| find_named_child_kind(node, "block"))
    else {
        return;
    };
    let statements = meaningful_statements(body);
    if is_empty_catch(analyzer.source, &statements) {
        analyzer.emit(Diag::new(
            node,
            "BCS013",
            "empty exception handling is forbidden",
            ("empty_catch", "handle_or_rethrow"),
        ));
        return;
    }
    check_verbose_rethrow(analyzer, node, &statements);
    if check_catch_all(analyzer, node, &statements) {
        return;
    }
    check_forged_returns(analyzer, &statements);
}

fn is_empty_catch(source: &str, statements: &[Node]) -> bool {
    statements.is_empty()
        || statements
            .iter()
            .all(|stmt| node_text(source, *stmt).trim() == "pass")
}

fn check_verbose_rethrow(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    statements: &[Node],
) {
    if statements.len() == 1
        && is_verbose_rethrow(analyzer.source, node, statements[0])
    {
        analyzer.emit_taste(
            Diag::new(
                node,
                "BCS013",
                "redundant exception rethrow; use bare raise/throw or remove \
                 the handler",
                ("verbose_rethrow", "bare_rethrow_or_delete"),
            ),
            "control-flow",
        );
    }
}

fn check_catch_all(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    statements: &[Node],
) -> bool {
    let catch_text = node_text(analyzer.source, node).trim();
    let catch_all = catch_text.starts_with("except:")
        || catch_text.starts_with("except Exception")
        || catch_text.contains("catch (...)");
    let rethrow_only = statements.iter().all(|stmt| {
        matches!(node_text(analyzer.source, *stmt).trim(), "raise" | "throw;")
    });
    if !catch_all || rethrow_only {
        return false;
    }
    if statements.len() == 1
        && is_silent_abort(node_text(analyzer.source, statements[0]).trim())
    {
        let text = node_text(analyzer.source, statements[0]).trim();
        let violation = analyzer.emit(Diag::new(
            node,
            "BCS013",
            "catch-all handler must not silently swallow errors with bare \
             return/continue/break",
            (text, "log_or_narrow_exception_or_rethrow"),
        ));
        violation.mark_high_confidence_violation();
        violation.category = Some("control-flow".to_owned());
        return true;
    }
    let violation = analyzer.emit(Diag::new(
        node,
        "BCS013",
        "catch-all handlers must rethrow or translate at a system boundary",
        ("unrestricted_catch_all", "rethrow_or_translate"),
    ));
    violation.severity = "warning".to_owned();
    violation.confidence = "low".to_owned();
    false
}

fn is_silent_abort(text: &str) -> bool {
    matches!(
        text,
        "return" | "return;" | "continue" | "continue;" | "break" | "break;"
    )
}

fn check_forged_returns(analyzer: &mut Analyzer<'_>, statements: &[Node]) {
    for &statement in statements {
        let text = node_text(analyzer.source, statement).trim();
        if is_forged_success_return(text) {
            let violation = analyzer.emit(Diag::new(
                statement,
                "BCS013",
                "catch blocks must not return a forged success result",
                (text, "handle_or_rethrow"),
            ));
            violation.severity = "warning".to_owned();
            violation.confidence = "low".to_owned();
        }
    }
}

fn is_forged_success_return(text: &str) -> bool {
    if matches!(
        text,
        "return true;"
            | "return true"
            | "return 0;"
            | "return True"
            | "return 0"
            | "return"
    ) {
        return true;
    }
    let Some(rest) = text.strip_prefix("return ") else {
        return false;
    };
    let compact: String = rest
        .trim_end_matches(';')
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    matches!(
        compact.as_str(),
        "{\"ok\":True}" | "{'ok':True}" | "{ok:true}" | "{\"ok\":true}"
    )
}

fn is_verbose_rethrow(source: &str, handler: Node, statement: Node) -> bool {
    if !matches!(statement.kind(), "raise_statement" | "throw_statement") {
        return false;
    }
    let Some(argument) = meaningful_statements(statement).into_iter().next()
    else {
        return false;
    };
    if argument.kind() != "identifier" {
        return false;
    }
    let name = node_text(source, argument);
    if handler.kind() == "except_clause" {
        let alias = handler.child_by_field_name("alias").or_else(|| {
            let value = handler.child_by_field_name("value")?;
            (value.kind() == "as_pattern")
                .then(|| value.child_by_field_name("alias"))
                .flatten()
        });
        return alias
            .is_some_and(|node| node_text(source, node).trim() == name);
    }
    handler
        .child_by_field_name("parameter")
        .or_else(|| find_named_child_kind(handler, "parameter_declaration"))
        .is_some_and(|param| node_text(source, param).contains(name))
}
