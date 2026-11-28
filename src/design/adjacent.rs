use tree_sitter::Node;

use crate::analyze::{is_python_keyword_only, node_text, parameter_name};
use crate::design::DesignContext;
use crate::language::Language;
use crate::model::Violation;

pub(crate) fn analyze_adjacent_same_type_params(
    ctx: &mut DesignContext<'_>,
    fn_name: Option<&str>,
    params_node: Node,
    params: &[Node],
) {
    if !matches!(
        ctx.language,
        Language::Python | Language::TypeScript | Language::Tsx
    ) || fn_name.is_some_and(is_math_or_symmetric_fn)
    {
        return;
    }
    let positional: Vec<Node> = params
        .iter()
        .copied()
        .filter(|p| is_plain_positional(ctx.language, ctx.source, *p))
        .collect();
    if positional.len() < 3 {
        return;
    }
    let typed: Vec<(String, Option<&'static str>)> = positional
        .iter()
        .map(|p| {
            let name = parameter_name(ctx.source, *p).unwrap_or_default();
            (name, primitive_param_type(ctx.language, ctx.source, *p))
        })
        .collect();
    for pair in typed.windows(2) {
        let [(name_a, Some(type_a)), (name_b, Some(type_b))] = pair else {
            continue;
        };
        if type_a != type_b || is_symmetric_param_pair(name_a, name_b) {
            continue;
        }
        emit_adjacent_violation(
            ctx,
            params_node,
            (name_a, type_a),
            (name_b, type_b),
        );
        break;
    }
}

fn is_plain_positional(language: Language, source: &str, param: Node) -> bool {
    !(language == Language::Python && is_python_keyword_only(source, param))
        && !matches!(
            param.kind(),
            "list_splat_pattern" | "dictionary_splat_pattern" | "rest_pattern"
        )
}

fn emit_adjacent_violation(
    ctx: &mut DesignContext<'_>,
    params_node: Node,
    first: (&str, &str),
    second: (&str, &str),
) {
    let (name_a, type_a) = first;
    let (name_b, type_b) = second;
    let mut violation = Violation::new(
        "BCS004",
        ctx.relative,
        params_node.start_position().row + 1,
        params_node.start_position().column + 1,
        "adjacent positional parameters share the same primitive type (easy to swap at call sites)",
        format!("{name_a}: {type_a}, {name_b}: {type_b}"),
        "keyword_only_or_named_options",
        false,
    );
    violation.mark_taste_warning("state-gain");
    violation.prescription = Some(if ctx.language == Language::Python {
        format!(
            "insert `*` before `{name_a}` and `{name_b}` to enforce keyword-only arguments"
        )
    } else {
        format!(
            "pass `{name_a}` and `{name_b}` in a typed options object so callers name them explicitly"
        )
    });
    ctx.violations.push(violation);
}

fn primitive_param_type(
    language: Language,
    source: &str,
    param: Node,
) -> Option<&'static str> {
    let text = node_text(source, param);
    if text.contains('=') {
        return None;
    }
    let after_colon = text.split_once(':')?.1.trim();
    let base = after_colon.split('=').next().unwrap_or("").trim();
    match language {
        Language::Python => match base {
            "int" => Some("int"),
            "str" => Some("str"),
            "float" => Some("float"),
            "bool" => Some("bool"),
            _ => None,
        },
        Language::TypeScript | Language::Tsx => match base {
            "number" => Some("number"),
            "string" => Some("string"),
            "boolean" => Some("boolean"),
            _ => None,
        },
        _ => None,
    }
}

fn is_math_or_symmetric_fn(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "clamp", "lerp", "min", "max", "add", "sub", "mul", "div", "swap",
        "rect", "box", "rgb", "rgba", "hsl", "color", "point", "vec", "range",
        "slice", "replace", "compare",
    ]
    .iter()
    .any(|word| lower.contains(word))
}

fn is_symmetric_param_pair(a: &str, b: &str) -> bool {
    let la = a.to_ascii_lowercase();
    let lb = b.to_ascii_lowercase();
    matches!(
        (la.as_str(), lb.as_str()),
        ("a", "b")
            | ("b", "c")
            | ("x", "y")
            | ("y", "z")
            | ("x1", "y1")
            | ("x2", "y2")
            | ("left", "right")
            | ("start", "end")
            | ("begin", "end")
            | ("low", "high")
            | ("lo", "hi")
            | ("min", "max")
            | ("row", "col")
            | ("width", "height")
            | ("w", "h")
            | ("src", "dst")
            | ("old", "new")
            | ("before", "after")
            | ("first", "second")
            | ("second", "third")
            | ("third", "fourth")
            | ("key", "value")
            | ("host", "port")
            | ("major", "minor")
            | ("minor", "patch")
            | ("line", "column")
            | ("line", "col")
            | ("top", "left")
            | ("top", "bottom")
            | ("code", "message")
            | ("name", "value")
            | ("entity_type", "entity_id")
    ) || (la.ends_with("_start") && lb.ends_with("_end"))
        || (la.ends_with("_min") && lb.ends_with("_max"))
        || (la.ends_with("_low") && lb.ends_with("_high"))
}
