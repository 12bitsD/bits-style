use tree_sitter::Node;

use super::ast::node_text;
use super::types::{Analyzer, Diag};

pub(crate) fn go_parameter_count(params: &[Node]) -> usize {
    params
        .iter()
        .map(|param| {
            let names = (0..param.named_child_count())
                .filter(|&i| {
                    param
                        .named_child(i as u32)
                        .is_some_and(|child| child.kind() == "identifier")
                })
                .count();
            names.max(1)
        })
        .sum()
}

pub(crate) fn analyze_go_context_param_order(
    analyzer: &mut Analyzer<'_>,
    params: &[Node],
) {
    let mut seen_non_context = false;
    for &param in params {
        let Some(ty) = param.child_by_field_name("type") else {
            continue;
        };
        if is_go_context_type(analyzer.source, ty) {
            if seen_non_context {
                analyzer.emit_taste(
                    Diag::new(
                        param,
                        "BCS004",
                        "context.Context must be the first parameter",
                        (node_text(analyzer.source, param), "context_first"),
                    ),
                    "level-purity",
                );
            }
        } else {
            seen_non_context = true;
        }
    }
}

fn is_go_context_type(source: &str, node: Node) -> bool {
    node.kind() == "qualified_type"
        && node_text(source, node)
            .rsplit('.')
            .next()
            .is_some_and(|name| name == "Context")
        && node.child_by_field_name("package").is_some_and(|pkg| {
            matches!(node_text(source, pkg), "context" | "stdctx")
        })
}

pub(crate) fn analyze_go_context_field(
    analyzer: &mut Analyzer<'_>,
    node: Node,
) {
    let imports_context = analyzer
        .result
        .imports
        .iter()
        .any(|import| import.specifier == "context");
    if !imports_context {
        return;
    }
    let Some(ty) = node.child_by_field_name("type") else {
        return;
    };
    let is_ctx_field = ty.kind() == "qualified_type"
        && node_text(analyzer.source, ty).ends_with(".Context")
        && ty.child_by_field_name("package").is_some_and(|pkg| {
            let pkg_name = node_text(analyzer.source, pkg);
            analyzer.result.imports.iter().any(|import| {
                import.specifier == "context"
                    && import.binding.as_deref() == Some(pkg_name)
            })
        });
    if is_ctx_field {
        analyzer.emit_taste(
            Diag::new(
                node,
                "BCS011",
                "context.Context must not be stored in structs",
                (node_text(analyzer.source, node), "pass_context_explicitly"),
            ),
            "state-gain",
        );
    }
}
