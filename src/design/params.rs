use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

use crate::analyze::{is_function, node_text, parameter_name, visit};
use crate::design::DesignContext;
use crate::language::Language;
use crate::model::{Position, Range as ModelRange, Violation};

pub(crate) fn analyze_tramp_data(
    ctx: &mut DesignContext<'_>,
    function: Node,
    params: &[Node],
) {
    if !(ctx.language == Language::Python || ctx.language.is_script())
        || params.len() < 5
    {
        return;
    }
    let Some(body) = function.child_by_field_name("body") else {
        return;
    };
    let param_names: BTreeSet<String> = params
        .iter()
        .filter_map(|p| parameter_name(ctx.source, *p))
        .collect();
    if param_names.len() < 5 {
        return;
    }
    let (uses, calls) =
        scan_tramp_body(ctx.source, function.id(), body, &param_names);
    if let Some(hit) = best_tramp_callee(&uses, calls, param_names.len()) {
        emit_tramp_violation(ctx, function, &hit);
    }
}

struct TrampHit {
    callee: String,
    tramp: Vec<String>,
    param_total: usize,
}

type TrampScan = (
    BTreeMap<String, usize>,
    BTreeMap<(usize, String), BTreeSet<String>>,
);

fn scan_tramp_body(
    source: &str,
    fn_id: usize,
    body: Node,
    param_names: &BTreeSet<String>,
) -> TrampScan {
    let mut total_uses: BTreeMap<String, usize> = BTreeMap::new();
    let mut call_params: BTreeMap<(usize, String), BTreeSet<String>> =
        BTreeMap::new();
    visit(body, &mut |node| {
        if node.id() != fn_id && is_function(node.kind()) {
            return;
        }
        if node.kind() == "identifier" {
            let name = node_text(source, node);
            if param_names.contains(name) && !is_member_or_kwarg_key(node) {
                *total_uses.entry(name.to_owned()).or_default() += 1;
            }
        }
        record_call_forwarded(source, node, param_names, &mut call_params);
    });
    (total_uses, call_params)
}

fn record_call_forwarded(
    source: &str,
    node: Node,
    param_names: &BTreeSet<String>,
    call_params: &mut BTreeMap<(usize, String), BTreeSet<String>>,
) {
    if !matches!(node.kind(), "call" | "call_expression" | "new_expression") {
        return;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    let callee = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("constructor"))
        .map(|c| node_text(source, c).trim().to_owned())
        .unwrap_or_else(|| "call".to_owned());
    let entry = call_params.entry((node.id(), callee)).or_default();
    collect_direct_forwarded_args(source, args, param_names, entry);
}

fn best_tramp_callee(
    total_uses: &BTreeMap<String, usize>,
    call_params: BTreeMap<(usize, String), BTreeSet<String>>,
    param_total: usize,
) -> Option<TrampHit> {
    let mut best: Option<TrampHit> = None;
    for ((_, callee), forwarded) in call_params {
        if callee.starts_with("super(") || callee.starts_with("super.") {
            continue;
        }
        let mut tramp: Vec<String> = forwarded
            .into_iter()
            .filter(|n| total_uses.get(n).copied().unwrap_or(0) == 1)
            .collect();
        if tramp.len() >= 4
            && tramp.len() * 2 >= param_total
            && best
                .as_ref()
                .is_none_or(|prev| tramp.len() > prev.tramp.len())
        {
            tramp.sort();
            best = Some(TrampHit {
                callee,
                tramp,
                param_total,
            });
        }
    }
    best
}

fn emit_tramp_violation(
    ctx: &mut DesignContext<'_>,
    function: Node,
    hit: &TrampHit,
) {
    let mut violation = Violation::new(
        "BCS004",
        ctx.relative,
        function.start_position().row + 1,
        function.start_position().column + 1,
        "function forwards most parameters untouched to a single callee (tramp data)",
        format!(
            "{}/{} params -> {}",
            hit.tramp.len(),
            hit.param_total,
            hit.callee
        ),
        "pass_constructed_object_or_inline",
        false,
    );
    violation.range = ModelRange {
        start: Position::new(
            function.start_position().row + 1,
            function.start_position().column + 1,
        ),
        end: Position::new(
            function.end_position().row + 1,
            function.end_position().column + 1,
        ),
    };
    violation.mark_taste_warning("level-purity");
    violation.prescription = Some(format!(
        "pass a pre-built object or construct {} at the caller instead of forwarding ({})",
        hit.callee,
        hit.tramp.join(", ")
    ));
    ctx.violations.push(violation);
}

fn is_member_or_kwarg_key(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    match parent.kind() {
        "keyword_argument" => parent
            .child_by_field_name("name")
            .is_some_and(|n| n.id() == node.id()),
        "attribute" | "member_expression" => parent
            .child_by_field_name("attribute")
            .or_else(|| parent.child_by_field_name("property"))
            .is_some_and(|n| n.id() == node.id()),
        "pair" => parent
            .child_by_field_name("key")
            .is_some_and(|n| n.id() == node.id()),
        _ => false,
    }
}

fn collect_direct_forwarded_args(
    source: &str,
    args: Node,
    param_names: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    for index in 0..args.named_child_count() {
        let Some(arg) = args.named_child(index as u32) else {
            continue;
        };
        match arg.kind() {
            "identifier" => insert_if_param(source, arg, param_names, out),
            "keyword_argument" => {
                if let Some(value) = arg.child_by_field_name("value") {
                    insert_if_param(source, value, param_names, out);
                }
            }
            "object" => collect_object_forwarded(source, arg, param_names, out),
            _ => {}
        }
    }
}

fn collect_object_forwarded(
    source: &str,
    object: Node,
    param_names: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    for j in 0..object.named_child_count() {
        let Some(prop) = object.named_child(j as u32) else {
            continue;
        };
        if prop.kind() == "shorthand_property_identifier" {
            let name = node_text(source, prop);
            if param_names.contains(name) {
                out.insert(name.to_owned());
            }
        } else if prop.kind() == "pair"
            && let Some(val) = prop.child_by_field_name("value")
        {
            insert_if_param(source, val, param_names, out);
        }
    }
}

fn insert_if_param(
    source: &str,
    node: Node,
    param_names: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    if node.kind() == "identifier" {
        let name = node_text(source, node);
        if param_names.contains(name) {
            out.insert(name.to_owned());
        }
    }
}
