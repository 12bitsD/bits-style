use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::Node;

use crate::language::Language;

use super::ast::{
    find_parameters, function_name, is_block, node_text, parameter_name,
};
use super::control_flow::analyze_opening_brace;
use super::function_params::{
    analyze_multi_out_params, analyze_parameter_mutability,
    effective_param_budget,
};
use super::go_checks::analyze_go_context_param_order;
use super::lines::code_line_count;
use super::metrics::{collect_locals, function_metrics, top_hotspots};
use super::naming::{
    is_constructor_name, is_framework_method_name, is_react_component,
    parameter_nodes, valid_function_name, valid_pascal_case,
};
use super::types::{Analyzer, Diag, FunctionMetrics};

pub(crate) fn analyze_function(analyzer: &mut Analyzer<'_>, node: Node) {
    check_function_brace(analyzer, node);
    let fn_name =
        function_name(node).map(|name| node_text(analyzer.source, name));
    let react_component = fn_name
        .is_some_and(|name| is_react_component(analyzer.language, node, name));
    check_function_size(analyzer, node);
    check_function_complexity(analyzer, node);
    check_function_parameters(analyzer, node, fn_name, react_component);
    check_function_name(analyzer, node, react_component);
    attach_function_explanation(analyzer, node);
}

fn check_function_brace(analyzer: &mut Analyzer<'_>, node: Node) {
    if matches!(
        analyzer.language,
        Language::Python | Language::Rust | Language::Go
    ) {
        return;
    }
    if let Some(body) = node.child_by_field_name("body")
        && is_block(body)
    {
        analyze_opening_brace(analyzer, node, body, true);
    }
}

fn check_function_size(analyzer: &mut Analyzer<'_>, node: Node) {
    let lines = code_line_count(analyzer, node);
    if lines > 48 {
        analyzer.emit_fn_warning(Diag::new(
            node,
            "BCS003",
            "function exceeds 48 code lines",
            (lines, "<=48"),
        ));
    }
    let locals = collect_locals(analyzer.language, analyzer.source, node);
    if locals.len() > 10 {
        analyzer.emit_fn_warning(Diag::new(
            node,
            "BCS003",
            "function has more than ten local variables",
            (locals.len(), "<=10"),
        ));
    }
}

fn check_function_complexity(analyzer: &mut Analyzer<'_>, node: Node) {
    let metrics = function_metrics(analyzer.source, node);
    emit_complexity_metrics(analyzer, node, &metrics);
}

fn emit_complexity_metrics(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    metrics: &FunctionMetrics,
) {
    if metrics.nesting > 3 {
        let hotspots = top_hotspots(&metrics.nesting_hotspots, 3);
        let violation = analyzer.emit_fn_warning(Diag::new(
            node,
            "BCS003",
            "control-flow nesting exceeds three levels",
            (metrics.nesting, "<=3"),
        ));
        violation.mark_high_confidence_violation();
        violation.hotspots = hotspots;
    }
    if metrics.cyclomatic > 10 {
        let hotspots = top_hotspots(&metrics.complexity_hotspots, 3);
        let violation = analyzer.emit_fn_warning(Diag::new(
            node,
            "BCS003",
            "cyclomatic complexity exceeds ten",
            (metrics.cyclomatic, "<=10"),
        ));
        violation.mark_high_confidence_violation();
        violation.hotspots = hotspots;
    }
    if metrics.cognitive > 15 {
        let hotspots = top_hotspots(&metrics.complexity_hotspots, 3);
        let violation = analyzer.emit_fn_warning(Diag::new(
            node,
            "BCS003",
            "cognitive complexity exceeds fifteen (cognitive_v1 shadow)",
            (metrics.cognitive, "<=15"),
        ));
        violation.severity = "warning".to_owned();
        violation.confidence = "high".to_owned();
        violation.category = Some("control-flow".to_owned());
        violation.hotspots = hotspots;
    }
}

fn check_function_parameters(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    fn_name: Option<&str>,
    react_component: bool,
) {
    let Some(parameters) = find_parameters(node) else {
        return;
    };
    let params =
        parameter_nodes(analyzer.language, analyzer.source, parameters);
    let (param_count, param_limit) = effective_param_budget(
        analyzer.language,
        analyzer.source,
        fn_name,
        &params,
    );
    if param_count > param_limit {
        let violation = analyzer.emit_fn_contract(Diag::new(
            parameters,
            "BCS004",
            "function has more than four parameters",
            (param_count, format!("<={param_limit}")),
        ));
        violation.severity = "warning".to_owned();
    }
    if analyzer.language == Language::Go {
        analyze_go_context_param_order(analyzer, &params);
        return;
    }
    check_non_go_parameters(analyzer, node, &params, react_component);
}

fn check_non_go_parameters(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    params: &[Node],
    react_component: bool,
) {
    let parameter_names: BTreeSet<String> = params
        .iter()
        .filter_map(|param| parameter_name(analyzer.source, *param))
        .collect();
    for &parameter in params {
        let text = node_text(analyzer.source, parameter);
        if is_positional_required_boolean(
            analyzer.language,
            analyzer.source,
            parameter,
            text,
        ) && !react_component
        {
            analyzer.emit_fn_contract(Diag::new(
                parameter,
                "BCS004",
                "boolean parameters must not switch between two behaviors",
                (text, "non_boolean_parameter"),
            ));
        }
        if analyzer.language.is_c_like() && is_unnamed_c_parameter(parameter) {
            analyzer.emit_fn_contract(Diag::new(
                parameter,
                "BCS004",
                "function declarations must include parameter names",
                (text, "named_parameter"),
            ));
        }
    }
    analyze_parameter_mutability(analyzer, node, &parameter_names);
    analyze_multi_out_params(analyzer, node, &parameter_names);
}

fn check_function_name(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    react_component: bool,
) {
    let Some(name_node) = function_name(node) else {
        return;
    };
    let name = node_text(analyzer.source, name_node);
    let allowed = is_constructor_name(node, analyzer.source, name)
        || (react_component && valid_pascal_case(name))
        || is_framework_method_name(analyzer.language, name)
        || valid_function_name(analyzer.language, name);
    if allowed {
        return;
    }
    let expected = if analyzer.language.is_script() {
        "lowerCamelCase"
    } else if analyzer.language == Language::Go {
        "MixedCase"
    } else {
        "lower_snake_case"
    };
    analyzer.emit(Diag::new(
        name_node,
        "BCS010",
        "function name does not match the language profile",
        (name, expected),
    ));
}

fn attach_function_explanation(analyzer: &mut Analyzer<'_>, node: Node) {
    let explanation =
        function_explanation(analyzer.source, &analyzer.result.comments, node);
    let line = node.start_position().row + 1;
    let relative = analyzer.relative;
    for violation in analyzer.result.violations.iter_mut().filter(|v| {
        v.rule_id == "BCS003"
            && v.file == relative
            && v.range.start.line == line
    }) {
        violation.explanation = explanation.clone();
    }
}

fn function_explanation(
    source: &str,
    comments: &[Range<usize>],
    function: Node,
) -> Option<String> {
    static EXPLANATION: OnceLock<Regex> = OnceLock::new();
    let regex = EXPLANATION.get_or_init(|| {
        Regex::new(r"bits:explain\(BCS003\):\s*(\S.*)").unwrap()
    });
    comments.iter().find_map(|range| {
        let text = source.get(range.clone())?;
        let line = source[..range.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        if line != function.start_position().row + 1
            && line != function.start_position().row
        {
            return None;
        }
        regex
            .captures(text)
            .map(|captures| captures[1].trim().to_owned())
    })
}

fn is_positional_required_boolean(
    language: Language,
    source: &str,
    parameter: Node,
    text: &str,
) -> bool {
    if !is_boolean_parameter(language, text) {
        return false;
    }
    if parameter.child_by_field_name("value").is_some()
        || parameter.child_by_field_name("default_value").is_some()
        || text.contains('=')
    {
        return false;
    }
    if language == Language::Python {
        return !has_python_star_before(source, parameter);
    }
    true
}

fn has_python_star_before(source: &str, parameter: Node) -> bool {
    let mut sibling = parameter.prev_sibling();
    while let Some(current) = sibling {
        if current.kind() == "list_splat_pattern"
            || node_text(source, current) == "*"
        {
            return true;
        }
        sibling = current.prev_sibling();
    }
    false
}

fn is_boolean_parameter(language: Language, text: &str) -> bool {
    if text.trim_start().starts_with(['{', '[']) {
        return false;
    }
    match language {
        Language::Python => text.contains(": bool") || text.contains(":bool"),
        _ if language.is_script() => {
            text.contains(": boolean") || text.contains(":boolean")
        }
        _ => text.trim_start().starts_with("bool ") || text.contains(" bool "),
    }
}

fn is_unnamed_c_parameter(node: Node) -> bool {
    let has_declarator = (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32).is_some_and(|child| {
            child.kind().contains("declarator") || child.kind() == "identifier"
        })
    });
    !has_declarator && node.kind() == "parameter_declaration"
}
