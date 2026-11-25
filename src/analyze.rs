use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::{Node, Tree};

use crate::language::Language;
use crate::model::{Coverage, Violation};

type IgnoredRanges<'a> = (&'a [Range<usize>], &'a [Range<usize>], &'a [Range<usize>]);

#[derive(Debug, Clone)]
pub struct ImportRecord {
    pub language: Language,
    pub specifier: String,
    pub binding: Option<String>,
    pub line: usize,
    pub column: usize,
    pub type_only: bool,
}

#[derive(Debug, Default)]
pub struct SyntaxAnalysis {
    pub violations: Vec<Violation>,
    pub strings: Vec<Range<usize>>,
    pub comments: Vec<Range<usize>>,
    pub jsx: Vec<Range<usize>>,
    pub type_ranges: Vec<Range<usize>>,
    pub interpolations: Vec<Range<usize>>,
    pub imports: Vec<ImportRecord>,
    pub annotation_strings: Vec<Range<usize>>,
    pub executed: BTreeSet<String>,
    pub partial: BTreeSet<String>,
    pub not_applicable: BTreeSet<String>,
}

pub fn analyze(language: Language, relative: &str, source: &str, tree: &Tree) -> SyntaxAnalysis {
    let mut result = SyntaxAnalysis::default();
    let root = tree.root_node();
    visit(root, &mut |node| {
        let kind = node.kind();
        if kind.contains("string") || matches!(kind, "raw_string_literal" | "template_string") {
            result.strings.push(node.byte_range());
        }
        if kind == "comment" {
            result.comments.push(node.byte_range());
        }
        if kind.starts_with("jsx_") {
            result.jsx.push(node.byte_range());
        }
        if matches!(
            kind,
            "type_alias_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "enum_specifier"
                | "struct_specifier"
        ) {
            result.type_ranges.push(node.byte_range());
        }
        if matches!(kind, "interpolation" | "template_substitution") {
            result.interpolations.push(node.byte_range());
        }
        if language == Language::Python
            && (kind == "type" || (kind == "assignment" && is_type_alias(language, source, node)))
        {
            visit(node, &mut |child| {
                if child.kind() == "string" {
                    result.annotation_strings.push(child.byte_range());
                }
            });
        }
        if language == Language::Python
            && kind == "call"
            && node
                .child_by_field_name("function")
                .is_some_and(|function| {
                    matches!(node_text(source, function), "cast" | "typing.cast")
                })
        {
            visit(node, &mut |child| {
                if child.kind() == "string" {
                    result.annotation_strings.push(child.byte_range());
                }
            });
        }
    });
    result.executed.extend(
        [
            "BCS003", "BCS004", "BCS005", "BCS006", "BCS010", "BCS011", "BCS014", "BCS016",
        ]
        .map(str::to_owned),
    );
    result.partial.extend(
        [
            "BCS002", "BCS003", "BCS004", "BCS005", "BCS006", "BCS008", "BCS009", "BCS010",
            "BCS011", "BCS012", "BCS013", "BCS014", "BCS016",
        ]
        .map(str::to_owned),
    );
    if language.is_c_like() {
        result.executed.insert("BCS012".to_owned());
    } else {
        result.not_applicable.insert("BCS012".to_owned());
    }
    if language == Language::Python
        || language.is_script()
        || language == Language::Cpp
        || language == Language::Rust
    {
        result.executed.insert("BCS013".to_owned());
    }
    if language == Language::Python {
        result
            .not_applicable
            .extend(["BCS007", "BCS008"].map(str::to_owned));
    } else {
        result
            .executed
            .extend(["BCS007", "BCS008"].map(str::to_owned));
    }
    result.executed.insert("BCS009".to_owned());

    visit(root, &mut |node| {
        if is_function(node.kind()) {
            analyze_function(
                language,
                relative,
                source,
                node,
                (&result.comments, &result.strings, &result.type_ranges),
                &mut result.violations,
            );
        }
        match node.kind() {
            "if_statement" | "if_expression" | "if_let_expression" => {
                analyze_if(language, relative, source, node, &mut result.violations)
            }
            "for_statement" | "while_statement" | "do_statement" | "for_in_statement"
            | "for_of_statement" | "loop_expression" | "while_expression" | "for_expression" => {
                analyze_loop(language, relative, source, node, &mut result.violations);
            }
            "switch_statement" | "match_expression" => {
                analyze_switch(language, relative, source, node, &mut result.violations)
            }
            "goto_statement" => analyze_goto(
                language,
                root,
                relative,
                source,
                node,
                &mut result.violations,
            ),
            "catch_clause" | "except_clause" => {
                analyze_catch(relative, source, node, &mut result.violations)
            }
            "preproc_function_def" => analyze_macro(relative, source, node, &mut result.violations),
            "preproc_if" | "preproc_ifdef" | "preproc_ifndef" => {
                if has_function_ancestor(node) {
                    push(
                        &mut result.violations,
                        relative,
                        node,
                        "BCS012",
                        "conditional compilation must wrap complete declarations or definitions",
                        "inside_function",
                        "complete_declaration",
                    );
                }
            }
            "import_statement"
            | "import_from_statement"
            | "import_declaration"
            | "preproc_include"
            | "use_declaration" => {
                analyze_import(language, relative, source, node, &mut result, true);
            }
            "class_definition"
            | "class_declaration"
            | "class_specifier"
            | "struct_specifier"
            | "interface_declaration"
            | "enum_specifier"
            | "enum_declaration"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "type_item" => {
                analyze_type(language, relative, source, node, &mut result.violations);
            }
            "call_expression" | "call" => {
                analyze_hidden_input(language, relative, source, node, &mut result.violations);
                if language.is_rust() {
                    analyze_rust_result(relative, source, node, &mut result.violations);
                }
                analyze_assert_call_side_effects(relative, source, node, &mut result.violations);
            }
            "assert_statement" => {
                if has_assert_side_effect(node) {
                    push_assert_side_effect(relative, node, &mut result.violations);
                }
            }
            "try_statement" => {
                analyze_try_rethrow(relative, source, node, &mut result.violations);
            }
            "binary_expression" => {
                analyze_binary_operator(relative, source, node, &mut result.violations);
            }
            "assignment_expression" => {
                if node
                    .child_by_field_name("right")
                    .is_some_and(|right| right.kind() == "assignment_expression")
                {
                    push(
                        &mut result.violations,
                        relative,
                        node,
                        "BCS006",
                        "chained assignment is forbidden",
                        "chained_assignment",
                        "separate_statements",
                    );
                }
            }
            "assignment"
                if language == Language::Python
                    && node
                        .child_by_field_name("right")
                        .is_some_and(|right| right.kind() == "assignment") =>
            {
                push(
                    &mut result.violations,
                    relative,
                    node,
                    "BCS006",
                    "chained assignment is forbidden",
                    "chained_assignment",
                    "separate_statements",
                );
            }
            "sequence_expression" | "comma_expression" => {
                push(
                    &mut result.violations,
                    relative,
                    node,
                    "BCS006",
                    "comma expressions must not replace braces",
                    "comma_expression",
                    "explicit_statements",
                );
            }
            "conditional_expression" | "ternary_expression"
                if node.parent().is_some_and(|parent| {
                    matches!(
                        parent.kind(),
                        "conditional_expression" | "ternary_expression"
                    )
                }) =>
            {
                push(
                    &mut result.violations,
                    relative,
                    node,
                    "BCS006",
                    "nested ternary expressions are forbidden",
                    "nested_ternary",
                    "explicit_branches",
                );
            }
            "unary_expression" | "not_operator" | "unary_operator"
                if is_double_negation(source, node) =>
            {
                push(
                    &mut result.violations,
                    relative,
                    node,
                    "BCS006",
                    "double-negation conditions are forbidden",
                    "double_negation",
                    "positive_condition",
                );
            }
            _ => {}
        }
        if is_variable_declaration(language, node) {
            analyze_variable(
                language,
                relative,
                source,
                root,
                node,
                &mut result.violations,
            );
        }
        if is_statement_list(node.kind()) {
            analyze_unreachable(relative, source, node, &mut result.violations);
        }
    });

    if language.is_c_like() {
        analyze_inline(root, relative, source, &mut result.violations);
    }
    result.jsx.sort_unstable_by_key(|range| range.start);
    analyze_spacing(
        language,
        relative,
        source,
        &result.strings,
        &result.comments,
        &result.jsx,
        &mut result.violations,
    );
    for violation in result
        .violations
        .iter_mut()
        .filter(|violation| matches!(violation.rule_id.as_str(), "BCS009" | "BCS010"))
    {
        violation.severity = "warning".to_owned();
        if violation.rule_id == "BCS009" && language == Language::Tsx {
            violation.confidence = "low".to_owned();
        }
    }
    result
}

#[derive(Debug, Clone)]
pub struct FunctionShape {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub nesting: usize,
    pub complexity: usize,
    pub cognitive: usize,
}

pub fn function_shapes(language: Language, source: &str, tree: &Tree) -> Vec<FunctionShape> {
    let _ = language;
    let mut shapes = Vec::new();
    visit(tree.root_node(), &mut |node| {
        if !is_function(node.kind()) {
            return;
        }
        let Some(name_node) = function_name(node) else {
            return;
        };
        let name = node_text(source, name_node).trim();
        if name.is_empty() || name.contains([' ', '(', ')', '=']) {
            return;
        }
        let metrics = function_metrics(source, node);
        shapes.push(FunctionShape {
            name: name.to_owned(),
            start_line: node.start_position().row + 1,
            end_line: node.end_position().row + 1,
            nesting: metrics.nesting,
            complexity: metrics.cyclomatic,
            cognitive: metrics.cognitive,
        });
    });
    shapes
}

pub fn analyze_imports(language: Language, source: &str, tree: &Tree) -> Vec<ImportRecord> {
    let mut result = SyntaxAnalysis::default();
    visit(tree.root_node(), &mut |node| {
        if matches!(
            node.kind(),
            "import_statement"
                | "import_from_statement"
                | "import_declaration"
                | "preproc_include"
                | "use_declaration"
        ) {
            analyze_import(language, "", source, node, &mut result, false);
        }
    });
    result.imports
}

fn analyze_function(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    ignored: IgnoredRanges,
    violations: &mut Vec<Violation>,
) {
    let (comments, strings, type_ranges) = ignored;
    if language != Language::Python
        && let Some(body) = node.child_by_field_name("body")
        && is_block(body)
    {
        analyze_opening_brace(language, relative, source, node, body, true, violations);
    }
    let react_component = function_name(node)
        .is_some_and(|name| is_react_component(language, node, node_text(source, name)));

    let lines = code_line_count(source, node, comments, strings, type_ranges);
    if lines > 48 {
        push_function(
            violations,
            relative,
            node,
            "BCS003",
            "function exceeds 48 code lines",
            lines,
            "<=48",
        );
    }

    let metrics = function_metrics(source, node);
    if metrics.nesting > 3 {
        push_function(
            violations,
            relative,
            node,
            "BCS003",
            "control-flow nesting exceeds three levels",
            metrics.nesting,
            "<=3",
        );
        if let Some(violation) = violations.last_mut() {
            violation.mark_high_confidence_violation();
        }
    }

    if metrics.cyclomatic > 10 {
        push_function(
            violations,
            relative,
            node,
            "BCS003",
            "cyclomatic complexity exceeds ten",
            metrics.cyclomatic,
            "<=10",
        );
        if let Some(violation) = violations.last_mut() {
            violation.mark_high_confidence_violation();
        }
    }

    if metrics.cognitive > 15 {
        push_function(
            violations,
            relative,
            node,
            "BCS003",
            "cognitive complexity exceeds fifteen (cognitive_v1 shadow)",
            metrics.cognitive,
            "<=15",
        );
        if let Some(violation) = violations.last_mut() {
            violation.severity = "warning".to_owned();
            violation.confidence = "high".to_owned();
            violation.category = Some("control-flow".to_owned());
        }
    }

    let locals = collect_locals(language, source, node);
    if locals.len() > 10 {
        push_function(
            violations,
            relative,
            node,
            "BCS003",
            "function has more than ten local variables",
            locals.len(),
            "<=10",
        );
    }

    if let Some(parameters) = find_parameters(node) {
        let params = parameter_nodes(language, parameters);
        if params.len() > 4 {
            push_function_contract(
                violations,
                relative,
                parameters,
                "BCS004",
                "function has more than four parameters",
                params.len(),
                "<=4",
            );
            if let Some(violation) = violations.last_mut() {
                violation.severity = "warning".to_owned();
            }
        }
        let parameter_names = params
            .iter()
            .filter_map(|parameter| parameter_name(source, *parameter))
            .collect::<BTreeSet<_>>();
        for parameter in params {
            let text = node_text(source, parameter);
            if is_positional_required_boolean(language, source, parameter, text) && !react_component
            {
                push_function_contract(
                    violations,
                    relative,
                    parameter,
                    "BCS004",
                    "boolean parameters must not switch between two behaviors",
                    text,
                    "non_boolean_parameter",
                );
            }
            if language.is_c_like() && is_unnamed_c_parameter(parameter) {
                push_function_contract(
                    violations,
                    relative,
                    parameter,
                    "BCS004",
                    "function declarations must include parameter names",
                    text,
                    "named_parameter",
                );
            }
        }
        analyze_parameter_mutability(
            language,
            relative,
            source,
            node,
            &parameter_names,
            violations,
        );
    }

    if let Some(name_node) = function_name(node) {
        let name = node_text(source, name_node);
        if !is_constructor_name(node, source, name)
            && !(react_component && valid_pascal_case(name))
            && !is_framework_method_name(language, name)
            && !valid_function_name(language, name)
        {
            let expected = if language.is_script() {
                "lowerCamelCase"
            } else {
                "lower_snake_case"
            };
            push(
                violations,
                relative,
                name_node,
                "BCS010",
                "function name does not match the language profile",
                name,
                expected,
            );
        }
    }
    let explanation = function_explanation(source, comments, node);
    for violation in violations.iter_mut().filter(|violation| {
        violation.rule_id == "BCS003"
            && violation.file == relative
            && violation.range.start.line == node.start_position().row + 1
    }) {
        violation.explanation = explanation.clone();
    }
}

fn function_explanation(source: &str, comments: &[Range<usize>], function: Node) -> Option<String> {
    static EXPLANATION: OnceLock<Regex> = OnceLock::new();
    let regex =
        EXPLANATION.get_or_init(|| Regex::new(r"bits:explain\(BCS003\):\s*(\S.*)").unwrap());
    comments.iter().find_map(|range| {
        let text = source.get(range.clone())?;
        let line = source[..range.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        if line != function.start_position().row + 1 && line != function.start_position().row {
            return None;
        }
        regex
            .captures(text)
            .map(|captures| captures[1].trim().to_owned())
    })
}

fn code_line_count(
    source: &str,
    function: Node,
    comments: &[Range<usize>],
    strings: &[Range<usize>],
    type_ranges: &[Range<usize>],
) -> usize {
    let mut ignored = comments.to_vec();
    ignored.extend_from_slice(strings);
    ignored.extend_from_slice(type_ranges);
    ignored.sort_unstable_by_key(|range| range.start);
    let mut lines = 0;
    let bytes = source.as_bytes();
    let mut line_start = function.start_byte();
    for row in function.start_position().row..=function.end_position().row {
        let line_end = if row == function.end_position().row {
            function.end_byte()
        } else {
            source[line_start..]
                .find('\n')
                .map_or(function.end_byte(), |index| line_start + index + 1)
        };
        let has_code = bytes[line_start..line_end]
            .iter()
            .enumerate()
            .any(|(index, byte)| {
                let offset = line_start + index;
                !byte.is_ascii_whitespace() && !inside_sorted_ranges(&ignored, offset)
            });
        lines += usize::from(has_code);
        line_start = line_end;
    }
    lines
}

fn inside_sorted_ranges(ranges: &[Range<usize>], offset: usize) -> bool {
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && ranges[..index].iter().any(|range| range.end > offset)
}

fn parameter_name(source: &str, parameter: Node) -> Option<String> {
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

fn analyze_parameter_mutability(
    language: Language,
    relative: &str,
    source: &str,
    function: Node,
    parameters: &BTreeSet<String>,
    violations: &mut Vec<Violation>,
) {
    if parameters.is_empty() {
        return;
    }
    visit(function, &mut |node| {
        if matches!(
            node.kind(),
            "assignment" | "assignment_expression" | "augmented_assignment"
        ) && let Some(target) = node
            .child_by_field_name("left")
            .or_else(|| node.child_by_field_name("target"))
            && target.kind() == "identifier"
        {
            let name = node_text(source, target);
            if parameters.contains(name) {
                push(
                    violations,
                    relative,
                    target,
                    "BCS011",
                    "function parameters must not be reassigned",
                    name,
                    "local_variable",
                );
            }
        }
        let declaration = match language {
            Language::C | Language::Cpp => matches!(node.kind(), "init_declarator" | "declaration"),
            Language::Python => false,
            _ => node.kind() == "variable_declarator",
        };
        if declaration
            && let Some(name) = parameter_name(source, node)
            && parameters.contains(&name)
        {
            push(
                violations,
                relative,
                node,
                "BCS011",
                "local variables must not shadow parameters",
                name,
                "distinct_name",
            );
        }
    });
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FunctionMetrics {
    pub nesting: usize,
    pub cyclomatic: usize,
    pub cognitive: usize,
}

pub(crate) fn function_metrics(source: &str, function: Node) -> FunctionMetrics {
    let root_name = function_name(function).map(|name| node_text(source, name));
    let mut metrics = FunctionMetrics {
        nesting: 0,
        cyclomatic: 1,
        cognitive: 0,
    };
    metrics_walk(function, function, source, root_name, 0, &mut metrics);
    metrics
}

fn is_else_if(node: Node) -> bool {
    matches!(
        node.kind(),
        "if_statement" | "if_expression" | "if_let_expression"
    ) && node
        .parent()
        .is_some_and(|parent| parent.kind() == "else_clause")
}

fn metrics_walk(
    node: Node,
    root: Node,
    source: &str,
    root_name: Option<&str>,
    depth: usize,
    metrics: &mut FunctionMetrics,
) {
    if node.id() != root.id() && is_function(node.kind()) {
        return;
    }
    let else_if = is_else_if(node);
    let nests = !else_if && is_nesting_construct(Language::C, node.kind());
    if nests {
        metrics.nesting = metrics.nesting.max(depth + 1);
    }
    let (cyclomatic, cognitive) = node_complexity_scores(node, source, root_name, depth, else_if);
    metrics.cyclomatic += cyclomatic;
    metrics.cognitive += cognitive;
    let next_depth = if nests { depth + 1 } else { depth };
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            metrics_walk(child, root, source, root_name, next_depth, metrics);
        }
    }
}

fn node_complexity_scores(
    node: Node,
    source: &str,
    root_name: Option<&str>,
    depth: usize,
    else_if: bool,
) -> (usize, usize) {
    let kind = node.kind();
    let structural = matches!(
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
            | "catch_clause"
            | "except_clause"
            | "conditional_expression"
            | "ternary_expression"
            | "switch_statement"
            | "match_statement"
            | "match_expression"
    );
    if structural {
        let cognitive = if else_if { 1 } else { 1 + depth };
        return (1, cognitive);
    }
    if kind == "elif_clause" || kind == "else_clause" {
        let cyclomatic = usize::from(kind == "elif_clause");
        return (cyclomatic, 1);
    }
    if kind == "match_arm" && node.child_by_field_name("guard").is_some() {
        return (0, 1);
    }
    if kind == "boolean_operator" {
        return (1, logical_sequence_increment(node));
    }
    if kind == "binary_expression"
        && let Some(operator) = node.child_by_field_name("operator")
        && matches!(operator.kind(), "&&" | "||" | "and" | "or")
    {
        return (1, logical_sequence_increment(node));
    }
    if kind == "goto_statement" {
        return (0, 1);
    }
    if matches!(kind, "call" | "call_expression")
        && let (Some(callee), Some(name)) = (node.child_by_field_name("function"), root_name)
        && callee.kind() == "identifier"
        && node_text(source, callee) == name
    {
        return (0, 1);
    }
    (0, 0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogicalOp {
    And,
    Or,
}

fn logical_operator_kind(node: Node) -> Option<LogicalOp> {
    match node.kind() {
        "boolean_operator" => {
            let has_or = (0..node.child_count())
                .any(|i| node.child(i).is_some_and(|child| child.kind() == "or"));
            Some(if has_or {
                LogicalOp::Or
            } else {
                LogicalOp::And
            })
        }
        "binary_expression" => {
            let operator = node.child_by_field_name("operator")?.kind();
            match operator {
                "&&" | "and" => Some(LogicalOp::And),
                "||" | "or" => Some(LogicalOp::Or),
                _ => None,
            }
        }
        _ => None,
    }
}

fn logical_sequence_increment(node: Node) -> usize {
    let Some(current) = logical_operator_kind(node) else {
        return 0;
    };
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if candidate.kind() == "parenthesized_expression" {
            parent = candidate.parent();
            continue;
        }
        return match logical_operator_kind(candidate) {
            Some(parent_op) if parent_op == current => 0,
            _ => 1,
        };
    }
    1
}

fn collect_locals(language: Language, source: &str, function: Node) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    visit(function, &mut |node| {
        if node.id() != function.id() && is_function(node.kind()) {
            return;
        }
        let collects = match language {
            Language::Python => matches!(
                node.kind(),
                "assignment" | "for_statement" | "named_expression"
            ),
            Language::Rust => node.kind() == "let_declaration",
            _ if language.is_script() => node.kind() == "variable_declarator",
            _ => matches!(node.kind(), "init_declarator" | "declarator"),
        };
        if !collects {
            return;
        }
        let target = if language == Language::Rust {
            node.child_by_field_name("pattern")
        } else if language == Language::Python {
            node.child_by_field_name("left")
                .or_else(|| node.child_by_field_name("target"))
        } else {
            node.child_by_field_name("name")
                .or_else(|| node.child_by_field_name("declarator"))
        };
        if let Some(target) = target {
            collect_pattern_identifiers(source, target, &mut names);
        } else if node.kind() == "declarator"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "declaration")
            && node.kind() == "identifier"
        {
            names.insert(node_text(source, node).to_owned());
        }
    });
    names.remove("self");
    names.remove("this");
    names
}

fn collect_pattern_identifiers(source: &str, node: Node, names: &mut BTreeSet<String>) {
    if matches!(
        node.kind(),
        "identifier" | "shorthand_property_identifier" | "shorthand_property_identifier_pattern"
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

fn analyze_binary_operator(
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let Some(right) = node.child_by_field_name("right") else {
        return;
    };
    if left.end_position().row != right.start_position().row {
        return;
    }
    let gap = &source[left.end_byte()..right.start_byte()];
    let operator = node
        .child_by_field_name("operator")
        .map(|operator| node_text(source, operator))
        .unwrap_or_else(|| gap.trim());
    if operator.is_empty() {
        return;
    }
    let expected = format!(" {operator} ");
    if gap != expected {
        push(
            violations,
            relative,
            node,
            "BCS009",
            "binary operators must have one space on each side",
            gap,
            expected,
        );
    }
}

fn analyze_if(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    let consequence = node.child_by_field_name("consequence");
    let alternative = node.child_by_field_name("alternative");
    if let (Some(then), Some(otherwise)) = (consequence, alternative)
        && ends_with_terminator(source, then)
        && !otherwise.kind().contains("if")
    {
        push(
            violations,
            relative,
            otherwise,
            "BCS005",
            "else after a terminating branch is forbidden",
            "redundant_else",
            "early_return",
        );
    }

    if language != Language::Python
        && let Some(then) = consequence
    {
        if is_block(then) {
            analyze_opening_brace(language, relative, source, node, then, false, violations);
        } else {
            push(
                violations,
                relative,
                then,
                "BCS007",
                "control-flow branches must use braces",
                "unbraced_branch",
                "braced_branch",
            );
        }
        if let Some(otherwise) = alternative {
            let else_body = if otherwise.kind() == "else_clause" {
                meaningful_statements(otherwise)
                    .into_iter()
                    .find(|child| is_block(*child) || child.kind() == "if_statement")
                    .unwrap_or(otherwise)
            } else {
                otherwise
            };
            if is_block(else_body) {
                analyze_opening_brace(
                    language, relative, source, otherwise, else_body, false, violations,
                );
            } else if else_body.kind() != "if_statement" {
                push(
                    violations,
                    relative,
                    else_body,
                    "BCS007",
                    "control-flow branches must use braces",
                    "unbraced_branch",
                    "braced_branch",
                );
            }
        }
    }

    if let Some(condition) = node.child_by_field_name("condition")
        && contains_assignment_condition(condition)
    {
        push(
            violations,
            relative,
            condition,
            "BCS006",
            "assignment inside a condition is forbidden",
            "condition_assignment",
            "separate_statement",
        );
    }

    analyze_collapsible_if(relative, node, violations);
    if let Some((branches, exhaustive)) = collect_if_branches(node) {
        analyze_identical_adjacent_arms(relative, source, &branches, violations);
        if exhaustive {
            analyze_branch_head_duplication(relative, source, node, &branches, violations);
        }
    }
}

fn analyze_collapsible_if(relative: &str, node: Node, violations: &mut Vec<Violation>) {
    if node.kind() == "if_let_expression"
        || is_else_if(node)
        || node.child_by_field_name("initializer").is_some()
        || node.child_by_field_name("alternative").is_some()
        || (0..node.named_child_count()).any(|i| {
            node.named_child(i as u32)
                .is_some_and(|c| matches!(c.kind(), "elif_clause" | "else_clause"))
        })
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
        || (0..inner.named_child_count()).any(|i| {
            inner
                .named_child(i as u32)
                .is_some_and(|c| matches!(c.kind(), "elif_clause" | "else_clause"))
        })
    {
        return;
    }
    let mut violation = Violation::new(
        "BCS005",
        relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        "collapsible nested if statements",
        "nested_if",
        "combined_condition",
        false,
    );
    violation.mark_taste_warning("control-flow");
    violations.push(violation);
}

fn analyze_identical_adjacent_arms(
    relative: &str,
    source: &str,
    branches: &[(Option<Node>, Node)],
    violations: &mut Vec<Violation>,
) {
    for pair in branches.windows(2) {
        let [(Some(cond_a), body_a), (Some(cond_b), body_b)] = pair else {
            continue;
        };
        if has_type_narrowing(source, *cond_a) || has_type_narrowing(source, *cond_b) {
            continue;
        }
        if expression_key(source, *body_a) != expression_key(source, *body_b) {
            continue;
        }
        let mut violation = Violation::new(
            "BCS016",
            relative,
            cond_a.start_position().row + 1,
            cond_a.start_position().column + 1,
            "adjacent branches have identical bodies",
            "identical_branch_bodies",
            "merged_condition",
            false,
        );
        violation.mark_taste_warning("symmetry");
        violations.push(violation);
    }
}

fn has_type_narrowing(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    if text.contains("typeof ") || text.contains("isinstance(") || text.contains(":=") {
        return true;
    }
    matches!(node.kind(), "if_let_expression" | "let_condition")
}

type IfBranches<'a> = Vec<(Option<Node<'a>>, Node<'a>)>;

fn collect_if_branches(node: Node<'_>) -> Option<(IfBranches<'_>, bool)> {
    if is_else_if(node) {
        return None;
    }
    let mut branches = Vec::new();
    let mut current = node;
    let exhaustive = loop {
        let condition = current.child_by_field_name("condition");
        let consequence = current.child_by_field_name("consequence")?;
        branches.push((condition, consequence));
        if (0..current.named_child_count()).any(|i| {
            current
                .named_child(i as u32)
                .is_some_and(|c| matches!(c.kind(), "elif_clause" | "else_clause"))
        }) {
            let mut has_else = false;
            for index in 0..current.named_child_count() {
                let Some(child) = current.named_child(index as u32) else {
                    continue;
                };
                match child.kind() {
                    "elif_clause" => {
                        branches.push((
                            child.child_by_field_name("condition"),
                            child.child_by_field_name("consequence")?,
                        ));
                    }
                    "else_clause" => {
                        branches.push((None, meaningful_statements(child).into_iter().next()?));
                        has_else = true;
                    }
                    _ => {}
                }
            }
            break has_else;
        }
        let alternative = current.child_by_field_name("alternative");
        match alternative.map(|alt| alt.kind()) {
            Some("if_statement" | "if_expression") => current = alternative?,
            Some("else_clause") => {
                let alt = alternative?;
                let child = meaningful_statements(alt).into_iter().next()?;
                if matches!(child.kind(), "if_statement" | "if_expression") {
                    current = child;
                } else {
                    branches.push((None, child));
                    break true;
                }
            }
            Some(_) => {
                branches.push((None, alternative?));
                break true;
            }
            None => break false,
        }
    };
    Some((branches, exhaustive))
}

fn analyze_branch_head_duplication(
    relative: &str,
    source: &str,
    node: Node,
    branches: &[(Option<Node>, Node)],
    violations: &mut Vec<Violation>,
) {
    if branches.len() < 2 {
        return;
    }
    let mut calls: Vec<(String, String)> = Vec::new();
    for (condition, body) in branches {
        if condition.is_some_and(has_expression_side_effect) {
            return;
        }
        let Some(call) = leading_pure_call(source, *body) else {
            return;
        };
        calls.push(call);
    }
    let Some((first_key, first_display)) = calls.first() else {
        return;
    };
    if !calls.iter().all(|(key, _)| key == first_key) {
        return;
    }
    let mut violation = Violation::new(
        "BCS016",
        relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        "mutually exclusive branches repeat the same leading call",
        first_display.clone(),
        "hoist_shared_setup_or_bind_handler",
        false,
    );
    violation.range = crate::model::Range {
        start: crate::model::Position::new(
            node.start_position().row + 1,
            node.start_position().column + 1,
        ),
        end: crate::model::Position::new(
            node.end_position().row + 1,
            node.end_position().column + 1,
        ),
    };
    violation.mark_taste_warning("symmetry");
    violations.push(violation);
}

fn leading_pure_call(source: &str, body: Node) -> Option<(String, String)> {
    let first = if is_block(body) {
        meaningful_statements(body).into_iter().next()?
    } else {
        body
    };
    let expr = if first.kind() == "expression_statement" {
        let mut children = meaningful_statements(first);
        if children.len() != 1 {
            return None;
        }
        children.remove(0)
    } else {
        first
    };
    let (is_await, call) = if matches!(expr.kind(), "await" | "await_expression") {
        let mut children = meaningful_statements(expr);
        if children.len() != 1 {
            return None;
        }
        (true, children.remove(0))
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
    let display = node_text(source, expr).trim().to_owned();
    Some((key, display))
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
    if kind.contains("call")
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
        )
    {
        return true;
    }
    (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32)
            .is_some_and(has_expression_side_effect)
    })
}

fn analyze_loop(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    if language != Language::Python
        && let Some(body) = node.child_by_field_name("body")
    {
        if is_block(body) {
            analyze_opening_brace(language, relative, source, node, body, false, violations);
        } else {
            push(
                violations,
                relative,
                body,
                "BCS007",
                "control-flow loops must use braces",
                "unbraced_loop",
                "braced_loop",
            );
        }
    }
    if let Some(condition) = node.child_by_field_name("condition")
        && contains_assignment_condition(condition)
    {
        push(
            violations,
            relative,
            condition,
            "BCS006",
            "assignment inside a condition is forbidden",
            "condition_assignment",
            "separate_statement",
        );
    }
}

fn analyze_opening_brace(
    language: Language,
    relative: &str,
    source: &str,
    parent: Node,
    body: Node,
    function: bool,
    violations: &mut Vec<Violation>,
) {
    let Some(brace) = first_token(source, body, '{') else {
        return;
    };
    let header_end = if function {
        find_parameters(parent)
            .map(|parameters| parameters.end_position().row)
            .unwrap_or(parent.start_position().row)
    } else {
        parent
            .child_by_field_name("condition")
            .map(|condition| condition.end_position().row)
            .unwrap_or(parent.start_position().row)
    };
    let next_line_required = function && language.is_c_like();
    if next_line_required && brace.row != header_end + 1 {
        push(
            violations,
            relative,
            body,
            "BCS007",
            "function opening brace must start on the next line",
            "same_line",
            "next_line",
        );
    } else if !next_line_required && brace.row != header_end {
        push(
            violations,
            relative,
            body,
            "BCS007",
            "opening brace must share the statement line",
            "next_line",
            "same_line",
        );
    }
}

fn analyze_switch(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    if let Some(body) = node.child_by_field_name("body") {
        analyze_opening_brace(language, relative, source, node, body, false, violations);
    }
    let switch_column = node.start_position().column;
    let case_column = if language.is_script() {
        switch_column + 2
    } else {
        switch_column
    };
    visit(node, &mut |child| {
        if !matches!(
            child.kind(),
            "case_statement" | "switch_case" | "default_statement" | "match_arm"
        ) {
            return;
        }
        if !language.is_rust() && child.start_position().column != case_column {
            push(
                violations,
                relative,
                child,
                "BCS008",
                "case/default indentation does not match the language profile",
                child.start_position().column,
                case_column,
            );
        }
        if language != Language::Rust
            && case_can_fallthrough(source, child)
            && !has_fallthrough_marker(source, child)
        {
            push(
                violations,
                relative,
                child,
                "BCS008",
                "intentional fallthrough must be explicitly marked",
                "unmarked_fallthrough",
                "fallthrough_marker",
            );
        }
    });
}

fn analyze_goto(
    language: Language,
    root: Node,
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    if language == Language::Cpp {
        push(
            violations,
            relative,
            node,
            "BCS005",
            "goto is forbidden in RAII-based languages",
            "goto",
            "raii_cleanup",
        );
        return;
    }
    let label = node_text(source, node)
        .trim()
        .trim_start_matches("goto")
        .trim_end_matches(';')
        .trim()
        .to_owned();
    if !(label.starts_with("out_") || label.starts_with("err_")) {
        push(
            violations,
            relative,
            node,
            "BCS005",
            "goto labels must use out_* or err_*",
            label.clone(),
            "out_*|err_*",
        );
    }
    let mut target = None;
    visit(root, &mut |candidate| {
        if candidate.kind() == "labeled_statement" {
            let text = node_text(source, candidate);
            if text.trim_start().starts_with(&(label.clone() + ":")) {
                target = Some(candidate);
            }
        }
    });
    if let Some(target) = target
        && target.start_byte() <= node.start_byte()
    {
        push(
            violations,
            relative,
            node,
            "BCS005",
            "goto must jump forward",
            "backward_jump",
            "forward_jump",
        );
    }
}

fn is_verbose_rethrow(source: &str, handler: Node, statement: Node) -> bool {
    let kind = statement.kind();
    if !matches!(kind, "raise_statement" | "throw_statement") {
        return false;
    }
    let Some(argument) = meaningful_statements(statement).into_iter().next() else {
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
        return alias.is_some_and(|node| node_text(source, node).trim() == name);
    }
    handler
        .child_by_field_name("parameter")
        .or_else(|| find_named_child_kind(handler, "parameter_declaration"))
        .is_some_and(|param| node_text(source, param).contains(name))
}

fn analyze_try_rethrow(relative: &str, source: &str, node: Node, violations: &mut Vec<Violation>) {
    let mut handlers = 0;
    let mut only_rethrows = true;
    for index in 0..node.named_child_count() {
        let Some(child) = node.named_child(index as u32) else {
            continue;
        };
        match child.kind() {
            "except_clause" | "catch_clause" => {
                handlers += 1;
                let Some(body) = child
                    .child_by_field_name("body")
                    .or_else(|| find_named_child_kind(child, "block"))
                else {
                    only_rethrows = false;
                    continue;
                };
                let statements = meaningful_statements(body);
                if statements.len() != 1
                    || !matches!(node_text(source, statements[0]).trim(), "raise" | "throw;")
                {
                    only_rethrows = false;
                }
            }
            "else_clause" | "finally_clause" => only_rethrows = false,
            _ => {}
        }
    }
    if handlers == 0 || !only_rethrows {
        return;
    }
    let mut violation = Violation::new(
        "BCS013",
        relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        "useless try block that only rethrows",
        "rethrow_only_try",
        "delete_try_wrapper",
        false,
    );
    violation.mark_taste_warning("control-flow");
    violations.push(violation);
}

fn has_assert_side_effect(node: Node) -> bool {
    let kind = node.kind();
    if matches!(
        kind,
        "named_expression"
            | "assignment"
            | "augmented_assignment"
            | "assignment_expression"
            | "update_expression"
    ) {
        return true;
    }
    (0..node.named_child_count()).any(|index| {
        node.named_child(index as u32)
            .is_some_and(|child| child.kind() != "comment" && has_assert_side_effect(child))
    })
}

fn push_assert_side_effect(relative: &str, node: Node, violations: &mut Vec<Violation>) {
    let mut violation = Violation::new(
        "BCS006",
        relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        "assertion condition must not contain explicit side effects",
        "assert_side_effect",
        "separate_statement_before_assert",
        false,
    );
    violation.mark_high_confidence_violation();
    violations.push(violation);
}

fn analyze_assert_call_side_effects(
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    let Some(callee) = node
        .child_by_field_name("function")
        .map(|function| node_text(source, function))
    else {
        return;
    };
    if !matches!(callee, "assert" | "static_assert" | "_Static_assert") {
        return;
    }
    if let Some(arguments) = node.child_by_field_name("arguments")
        && has_assert_side_effect(arguments)
    {
        push_assert_side_effect(relative, node, violations);
    }
}

fn in_cfg_test_module(source: &str, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item" {
            let mut sibling = parent.prev_named_sibling();
            while let Some(attr) = sibling {
                if attr.kind() != "attribute_item" {
                    break;
                }
                if node_text(source, attr).contains("cfg(test)") {
                    return true;
                }
                sibling = attr.prev_named_sibling();
            }
        }
        current = parent.parent();
    }
    false
}

fn analyze_catch(relative: &str, source: &str, node: Node, violations: &mut Vec<Violation>) {
    let Some(body) = node
        .child_by_field_name("body")
        .or_else(|| find_named_child_kind(node, "block"))
    else {
        return;
    };
    let statements = meaningful_statements(body);
    if statements.is_empty()
        || statements
            .iter()
            .all(|statement| node_text(source, *statement).trim() == "pass")
    {
        push(
            violations,
            relative,
            node,
            "BCS013",
            "empty exception handling is forbidden",
            "empty_catch",
            "handle_or_rethrow",
        );
        return;
    }
    let catch_text = node_text(source, node).trim();
    let catch_all = catch_text.starts_with("except:")
        || catch_text.starts_with("except Exception")
        || catch_text.contains("catch (...)");
    let rethrow_only = statements
        .iter()
        .all(|statement| matches!(node_text(source, *statement).trim(), "raise" | "throw;"));
    if statements.len() == 1 && is_verbose_rethrow(source, node, statements[0]) {
        let mut violation = Violation::new(
            "BCS013",
            relative,
            node.start_position().row + 1,
            node.start_position().column + 1,
            "redundant exception rethrow; use bare raise/throw or remove the handler",
            "verbose_rethrow",
            "bare_rethrow_or_delete",
            false,
        );
        violation.mark_taste_warning("control-flow");
        violations.push(violation);
    }
    if catch_all && !rethrow_only {
        push(
            violations,
            relative,
            node,
            "BCS013",
            "catch-all handlers must rethrow or translate at a system boundary",
            "unrestricted_catch_all",
            "rethrow_or_translate",
        );
        if let Some(violation) = violations.last_mut() {
            violation.severity = "warning".to_owned();
            violation.confidence = "low".to_owned();
        }
    }
    for statement in statements {
        let text = node_text(source, statement).trim();
        if matches!(
            text,
            "return true;"
                | "return false;"
                | "return 0;"
                | "return null;"
                | "return None"
                | "return True"
                | "return False"
                | "return 0"
                | "return"
        ) {
            push(
                violations,
                relative,
                statement,
                "BCS013",
                "catch blocks must not return a forged success result",
                text,
                "handle_or_rethrow",
            );
            if let Some(violation) = violations.last_mut() {
                violation.severity = "warning".to_owned();
                violation.confidence = "low".to_owned();
            }
        }
    }
}

fn analyze_rust_result(relative: &str, source: &str, node: Node, violations: &mut Vec<Violation>) {
    let text = node_text(source, node);
    if text.contains(".unwrap()") || text.contains(".expect(") {
        push(
            violations,
            relative,
            node,
            "BCS013",
            "Result must be handled explicitly instead of unwrap/expect",
            text,
            "use ? or explicit error handling",
        );
        if let Some(violation) = violations.last_mut() {
            violation.severity = "warning".to_owned();
            violation.confidence = "low".to_owned();
        }
    }
    if text.contains("panic!") || text.contains("unimplemented!") || text.contains("todo!") {
        push(
            violations,
            relative,
            node,
            "BCS013",
            "panic-based control flow requires an explicit invariant",
            text,
            "documented invariant or error result",
        );
        if let Some(violation) = violations.last_mut() {
            violation.severity = "warning".to_owned();
            violation.confidence = "low".to_owned();
        }
    }
}

fn analyze_macro(relative: &str, source: &str, node: Node, violations: &mut Vec<Violation>) {
    let text = node_text(source, node);
    for keyword in ["return", "goto", "break", "continue"] {
        if contains_word(text, keyword) {
            push(
                violations,
                relative,
                node,
                "BCS012",
                "function-like macros must not hide control flow",
                keyword,
                "explicit_control_flow",
            );
        }
    }
    let statements = text.matches(';').count();
    if statements > 1 && !(text.contains("do") && text.contains("while") && text.contains("(0)")) {
        push(
            violations,
            relative,
            node,
            "BCS012",
            "multi-statement macros must use a safe do-while block",
            "unwrapped_macro",
            "do_while_0",
        );
    }
}

fn analyze_inline(root: Node, relative: &str, source: &str, violations: &mut Vec<Violation>) {
    visit(root, &mut |node| {
        if !is_function(node.kind()) {
            return;
        }
        let text = node_text(source, node);
        if !contains_word(text, "inline") {
            return;
        }
        let lines = node.end_position().row - node.start_position().row + 1;
        if lines > 5 {
            push(
                violations,
                relative,
                node,
                "BCS012",
                "explicit inline function body exceeds three lines",
                lines.saturating_sub(2),
                "<=3",
            );
        }
        if contains_word(text, "static")
            && let Some(name) = function_name(node).map(|name| node_text(source, name))
            && source.match_indices(name).count() == 2
        {
            push(
                violations,
                relative,
                node,
                "BCS012",
                "single-use static functions must not be manually inlined",
                "single_use_inline",
                "compiler_decides",
            );
        }
    });
}

fn in_type_checking_block(source: &str, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "if_statement"
            && let Some(condition) = parent.child_by_field_name("condition")
        {
            let text = node_text(source, condition).trim();
            if text == "TYPE_CHECKING" || text.ends_with(".TYPE_CHECKING") {
                return true;
            }
        }
        current = parent.parent();
    }
    false
}

fn python_forward_annotation_ref(source: &str, strings: &[Range<usize>], name: &str) -> bool {
    strings.iter().any(|range| {
        source
            .get(range.clone())
            .is_some_and(|literal| contains_word(literal, name))
    })
}

fn in_python_main_guard(source: &str, node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "if_statement"
            && let Some(condition) = parent.child_by_field_name("condition")
        {
            let text = node_text(source, condition);
            if text.contains("__name__") && text.contains("__main__") {
                return true;
            }
        }
        if matches!(parent.kind(), "function_definition" | "class_definition") {
            return false;
        }
        current = parent.parent();
    }
    false
}

fn is_optional_import_fallback(source: &str, node: Node, value: &str) -> bool {
    if value != "None" {
        return false;
    }
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "except_clause" {
            let text = node_text(source, parent);
            return text.starts_with("except ImportError")
                || text.starts_with("except ModuleNotFoundError")
                || (text.starts_with("except (") && text.contains("ImportError"));
        }
        if parent.kind() == "try_statement" || parent.kind() == "function_definition" {
            return false;
        }
        current = parent.parent();
    }
    false
}

fn analyze_import(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    result: &mut SyntaxAnalysis,
    diagnostics: bool,
) {
    let text = node_text(source, node).trim();
    let masked = if diagnostics {
        mask_ranges(
            source,
            &result.strings,
            &result.comments,
            &result.interpolations,
        )
    } else {
        String::new()
    };
    let reexports = if diagnostics {
        python_reexports(source)
    } else {
        BTreeSet::new()
    };
    let type_checking_block = language == Language::Python && in_type_checking_block(source, node);
    let mut record = |specifier: String, binding: Option<String>, type_only: bool| {
        let type_only = type_only || type_checking_block;
        let init_reexport = language == Language::Python
            && relative.ends_with("__init__.py")
            && text.trim_start().starts_with("from .");
        let rust_allowed_wildcard = language == Language::Rust
            && (text.trim_start().starts_with("pub use ")
                || (text.contains("super::*") && in_cfg_test_module(source, node)));
        if diagnostics
            && (binding.as_deref() == Some("*") || text.contains("import *"))
            && !init_reexport
            && !rust_allowed_wildcard
        {
            push(
                &mut result.violations,
                relative,
                node,
                "BCS014",
                "wildcard imports are forbidden",
                text,
                "explicit_imports",
            );
        }
        let noqa_reexport = language == Language::Python
            && (text.contains("noqa: F401") || text.contains("noqa:F401"));
        if diagnostics
            && let Some(name) = binding.as_deref()
            && name != "*"
            && !noqa_reexport
            && !reexports.contains(name)
            && !is_likely_trait_name(name)
            && count_identifier(&masked, name) <= 1
            && !(language == Language::Python
                && python_forward_annotation_ref(source, &result.annotation_strings, name))
        {
            push(
                &mut result.violations,
                relative,
                node,
                "BCS014",
                "unused import",
                name,
                "used_import",
            );
        }
        result.imports.push(ImportRecord {
            language,
            specifier,
            binding,
            line: node.start_position().row + 1,
            column: node.start_position().column + 1,
            type_only,
        });
    };

    match language {
        Language::Rust => {
            let is_reexport = text.trim_start().starts_with("pub use ");
            let clause = text
                .trim_start()
                .trim_start_matches("pub use ")
                .trim_start_matches("use ")
                .trim_end_matches(';')
                .trim();
            let binding = clause
                .rsplit("::")
                .next()
                .unwrap_or(clause)
                .trim_matches(|character| matches!(character, '{' | '}' | ' '));
            if binding == "*" || clause.contains("::*") {
                record(clause.to_owned(), Some("*".to_owned()), false);
            } else if !is_reexport {
                for name in binding.split(',') {
                    let binding = name
                        .trim()
                        .trim_matches(|character| matches!(character, '{' | '}' | ' '));
                    let binding = binding.split(" as ").last().unwrap_or(binding).trim();
                    if binding.is_empty() || binding == "self" || binding == "_" {
                        continue;
                    }
                    record(clause.to_owned(), Some(binding.to_owned()), false);
                }
            }
        }
        Language::Python => {
            if text.starts_with("from ") {
                let mut parts = text.split_whitespace();
                let _from = parts.next();
                let module = parts.next().unwrap_or("");
                let imports = text.split_once(" import ").map_or("", |parts| parts.1);
                for name in imports.split(',') {
                    let name = name
                        .trim()
                        .trim_matches(|character| matches!(character, '(' | ')' | '\\'));
                    let binding = name.split(" as ").last().unwrap_or(name).trim();
                    if binding.is_empty() {
                        continue;
                    }
                    record(module.to_owned(), Some(binding.to_owned()), false);
                }
            } else {
                let imports = text.trim_start_matches("import ");
                for name in imports.split(',') {
                    let name = name.trim();
                    let module = name.split(" as ").next().unwrap_or(name).trim();
                    let binding = name.split(" as ").last().unwrap_or(name).trim();
                    record(module.to_owned(), Some(binding.to_owned()), false);
                }
            }
        }
        _ if language.is_script() => {
            let specifier = text
                .split_once(" from ")
                .and_then(|parts| parts.1.split(['\'', '"']).nth(1))
                .or_else(|| {
                    text.split_once("require(")
                        .and_then(|parts| parts.1.split(['\'', '"']).nth(1))
                })
                .unwrap_or("")
                .to_owned();
            let clause = text
                .split_once("import ")
                .and_then(|parts| parts.1.split_once(" from "))
                .map(|parts| parts.0)
                .unwrap_or("");
            let bindings = parse_js_bindings(clause);
            if bindings.is_empty() {
                record(specifier, None, false);
            } else {
                for (binding, type_only) in bindings {
                    record(specifier.clone(), Some(binding), type_only);
                }
            }
        }
        _ => {
            let specifier = text.split(['"', '<', '>']).nth(1).unwrap_or("").to_owned();
            record(specifier, None, false);
        }
    }
}

fn parse_js_bindings(clause: &str) -> Vec<(String, bool)> {
    let clause = clause.trim();
    let (clause, whole_type_only) = clause
        .strip_prefix("type ")
        .map_or((clause, false), |rest| (rest.trim(), true));
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut current = String::new();
    for character in clause.chars() {
        match character {
            '{' => {
                depth += 1;
                current.push(character);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(character),
        }
    }
    parts.push(current);

    let mut bindings = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.starts_with('*') {
            bindings.push(("*".to_owned(), whole_type_only));
            continue;
        }
        let mut candidates = Vec::new();
        if let Some((prefix, inside)) = part.split_once('{') {
            if !prefix.trim().is_empty() {
                candidates.push(prefix.trim());
            }
            if let Some((inside, _)) = inside.rsplit_once('}') {
                candidates.extend(inside.split(',').map(str::trim));
            }
        } else {
            candidates.push(part);
        }
        for candidate in candidates {
            let (candidate, item_type_only) = candidate
                .strip_prefix("type ")
                .map_or((candidate, false), |rest| (rest.trim(), true));
            let binding = candidate.split(" as ").last().unwrap_or(candidate).trim();
            if !binding.is_empty() {
                bindings.push((binding.to_owned(), whole_type_only || item_type_only));
            }
        }
    }
    bindings
}

fn is_likely_trait_name(name: &str) -> bool {
    matches!(
        name,
        "Write"
            | "Read"
            | "Iterator"
            | "Future"
            | "Error"
            | "Display"
            | "Debug"
            | "Clone"
            | "Copy"
            | "Send"
            | "Sync"
            | "Drop"
            | "Default"
            | "From"
            | "Into"
            | "TryFrom"
            | "TryInto"
            | "AsRef"
            | "AsMut"
            | "Deref"
            | "DerefMut"
            | "Borrow"
            | "BorrowMut"
            | "Fn"
            | "FnMut"
            | "FnOnce"
    )
}

fn python_reexports(source: &str) -> BTreeSet<String> {
    static REEXPORT: OnceLock<Regex> = OnceLock::new();
    let regex =
        REEXPORT.get_or_init(|| Regex::new(r#"[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']"#).unwrap());
    source
        .split("__all__")
        .skip(1)
        .flat_map(|suffix| {
            regex
                .captures_iter(suffix)
                .map(|capture| capture[1].to_owned())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn analyze_type(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    if let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| find_named_child_kind(node, "type_identifier"))
    {
        let text = node_text(source, name);
        if !valid_pascal_case(text) {
            push(
                violations,
                relative,
                name,
                "BCS010",
                "type names must use PascalCase",
                text,
                "PascalCase",
            );
        }
    }
    if language == Language::Cpp {
        let text = node_text(source, node);
        if text.contains("getInstance") && text.contains("static") {
            push(
                violations,
                relative,
                node,
                "BCS011",
                "singleton patterns are forbidden",
                "singleton",
                "explicit_dependencies",
            );
        }
    }
}

fn analyze_variable(
    language: Language,
    relative: &str,
    source: &str,
    root: Node,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    let _ = root;
    let global = !has_function_ancestor(node) && !has_type_ancestor(node);
    let text = node_text(source, node);
    if global {
        let mutable = match language {
            Language::Rust => {
                if node.kind() == "static_item" {
                    text.contains("static mut")
                } else {
                    node.kind() == "let_declaration"
                }
            }
            Language::Python => {
                let name = node
                    .child_by_field_name("left")
                    .map(|left| node_text(source, left).trim())
                    .unwrap_or("");
                let value = node
                    .child_by_field_name("right")
                    .map(|right| node_text(source, right).trim())
                    .unwrap_or("");
                let mutable_literal = value.starts_with('[')
                    || value.starts_with('{')
                    || ["list(", "dict(", "set(", "bytearray("]
                        .iter()
                        .any(|prefix| value.starts_with(prefix));
                node.kind() == "assignment"
                    && !is_type_alias(language, source, node)
                    && !name.is_empty()
                    && !name.contains([',', '(', '[', '.'])
                    && !name.starts_with('_')
                    && !value.starts_with("logging.getLogger(")
                    && !in_python_main_guard(source, node)
                    && !is_optional_import_fallback(source, node, value)
                    && (!valid_upper_snake(name) || mutable_literal)
            }
            _ if language.is_script() => {
                matches!(node.kind(), "lexical_declaration" | "variable_declaration")
                    && (text.trim_start().starts_with("let ")
                        || text.trim_start().starts_with("var "))
            }
            _ => {
                node.kind() == "declaration"
                    && !text.contains("const")
                    && !text.contains("static const")
            }
        };
        if mutable {
            push(
                violations,
                relative,
                node,
                "BCS011",
                "mutable global state is forbidden",
                text.lines().next().unwrap_or(text),
                "explicit_dependency",
            );
        }
    }

    if language == Language::Python && is_type_alias(language, source, node) {
        if let Some(target) = node.child_by_field_name("left") {
            let name = node_text(source, target);
            if !valid_pascal_case(name) {
                push(
                    violations,
                    relative,
                    target,
                    "BCS010",
                    "type aliases must use PascalCase",
                    name,
                    "PascalCase",
                );
            }
        }
        return;
    }
    if language == Language::Python && has_type_ancestor(node) {
        return;
    }
    if language == Language::Python && node.kind() == "assignment" {
        if let Some(target) = node.child_by_field_name("left")
            && target.kind() == "identifier"
        {
            let mut names = BTreeSet::new();
            collect_pattern_identifiers(source, target, &mut names);
            for name in names {
                if global && valid_upper_snake(&name) {
                    continue;
                }
                if !valid_lower_snake(&name) {
                    push(
                        violations,
                        relative,
                        target,
                        "BCS010",
                        "variable name does not match the language profile",
                        name,
                        "lower_snake_case",
                    );
                }
            }
        }
        return;
    }
    if let Some(name) = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("declarator"))
        .or_else(|| node.child_by_field_name("pattern"))
    {
        let name_text = node_text(source, name);
        if matches!(
            name.kind(),
            "identifier" | "shorthand_property_identifier_pattern"
        ) {
            let is_constant = (global
                && (text.trim_start().starts_with("const ")
                    || node.parent().is_some_and(|parent| {
                        node_text(source, parent).trim_start().starts_with("const ")
                    })))
                || node.kind() == "preproc_def";
            if is_constant
                && !(language.is_script()
                    && valid_pascal_case(name_text)
                    && is_react_component(language, node, name_text))
            {
                if !valid_upper_snake(name_text) && !valid_function_name(language, name_text) {
                    push(
                        violations,
                        relative,
                        name,
                        "BCS010",
                        "constants must use UPPER_SNAKE_CASE",
                        name_text,
                        "UPPER_SNAKE_CASE",
                    );
                }
            } else if !valid_function_name(language, name_text)
                && !(language.is_script()
                    && valid_pascal_case(name_text)
                    && is_react_component(language, node, name_text))
            {
                let expected = if language.is_script() {
                    "lowerCamelCase"
                } else {
                    "lower_snake_case"
                };
                push(
                    violations,
                    relative,
                    name,
                    "BCS010",
                    "variable name does not match the language profile",
                    name_text,
                    expected,
                );
            }
        }
    }
}

fn is_type_alias(language: Language, source: &str, node: Node) -> bool {
    if language != Language::Python || node.child_by_field_name("type").is_some() {
        return false;
    }
    let text = node_text(source, node);
    let name = text.split('=').next().unwrap_or("").trim();
    let value = text.split_once('=').map_or("", |parts| parts.1.trim());
    let prefixes = [
        "Annotated[",
        "Literal[",
        "Callable[",
        "Union[",
        "Optional[",
        "TypeVar(",
        "NewType(",
        "Type[",
        "list[",
        "dict[",
        "tuple[",
        "set[",
        "frozenset[",
        "Sequence[",
        "Mapping[",
        "MutableMapping[",
        "Iterable[",
        "Iterator[",
        "Awaitable[",
        "Coroutine[",
        "Protocol",
        "TypedDict",
        "NamedTuple",
        "TypeAlias",
        "TypeAliasType",
        "LiteralString",
        "Self",
        "ClassVar",
        "Final",
        "type ",
    ];
    let unqualified = value.split_once('.').map_or(value, |(module, rest)| {
        if module.chars().all(|c| c.is_alphanumeric() || c == '_') {
            rest
        } else {
            value
        }
    });
    if prefixes
        .iter()
        .any(|prefix| value.starts_with(prefix) || unqualified.starts_with(prefix))
    {
        return true;
    }
    let builtins = [
        "int", "str", "float", "bool", "bytes", "dict", "list", "set", "tuple", "Any", "Self",
    ];
    if builtins.contains(&value) {
        return true;
    }
    name.chars()
        .next()
        .is_some_and(|character| character.is_uppercase() || character == '_')
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_uppercase())
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}

fn analyze_hidden_input(
    language: Language,
    relative: &str,
    source: &str,
    node: Node,
    violations: &mut Vec<Violation>,
) {
    let text = node_text(source, node);
    let hidden = match language {
        Language::Python => text.contains("os.environ") || text.contains("os.getenv"),
        Language::Rust => {
            text.contains("env::var")
                || text.contains("SystemTime::now")
                || text.contains("Instant::now")
                || text.contains("rand::random")
                || text.contains("thread_rng")
        }
        _ if language.is_script() => text.contains("process.env"),
        _ => text.starts_with("getenv(") || text.contains("std::getenv"),
    };
    if hidden {
        push(
            violations,
            relative,
            node,
            "BCS011",
            "environment values must be passed explicitly",
            text.lines().next().unwrap_or(text),
            "explicit_input",
        );
    }
}

fn analyze_unreachable(relative: &str, source: &str, node: Node, violations: &mut Vec<Violation>) {
    let statements = meaningful_statements(node);
    for pair in statements.windows(2) {
        if is_statement(pair[0].kind())
            && is_statement(pair[1].kind())
            && pair[0].start_position().row == pair[1].start_position().row
        {
            push(
                violations,
                relative,
                pair[1],
                "BCS006",
                "each statement must be on its own line",
                "multiple_statements_per_line",
                "one_statement_per_line",
            );
        }
        if is_terminating(pair[0]) && is_statement(pair[1].kind()) {
            push(
                violations,
                relative,
                pair[1],
                "BCS014",
                "unreachable code is forbidden",
                node_text(source, pair[1]).lines().next().unwrap_or(""),
                "reachable_code",
            );
        }
    }
}

fn analyze_spacing(
    language: Language,
    relative: &str,
    source: &str,
    strings: &[Range<usize>],
    comments: &[Range<usize>],
    jsx: &[Range<usize>],
    violations: &mut Vec<Violation>,
) {
    let masked = mask_ranges(source, strings, comments, &[]);
    static KEYWORD: OnceLock<Regex> = OnceLock::new();
    static CALL: OnceLock<Regex> = OnceLock::new();
    static INNER: OnceLock<Regex> = OnceLock::new();
    let keyword = KEYWORD.get_or_init(|| Regex::new(r"\b(if|switch|for|while|catch)\(").unwrap());
    let call = CALL.get_or_init(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)[ \t]+\(").unwrap());
    let inner = INNER.get_or_init(|| Regex::new(r"\([ \t]+[^()\n]*[ \t]+\)").unwrap());
    let mut line_start = 0;
    for (index, line) in masked.lines().enumerate() {
        let line_number = index + 1;
        let line_end = line_start + line.len() + 1;
        if ranges_overlap(jsx, line_start, line_end) {
            line_start = line_end;
            continue;
        }
        if let Some(found) = keyword.find(line) {
            if found.start() > 0 && line[..found.start()].ends_with('.') {
                line_start = line_end;
                continue;
            }
            violations.push(Violation::new(
                "BCS009",
                relative,
                line_number,
                found.start() + 1,
                "control keyword must be followed by one space",
                &line[found.range()],
                "keyword (",
                false,
            ));
        }
        for found in call.captures_iter(line) {
            let name = found.get(1).map_or("", |capture| capture.as_str());
            if matches!(
                name,
                "if" | "switch"
                    | "for"
                    | "while"
                    | "catch"
                    | "return"
                    | "sizeof"
                    | "typeof"
                    | "alignof"
                    | "async"
                    | "await"
                    | "function"
                    | "new"
                    | "import"
                    | "export"
                    | "and"
                    | "or"
                    | "except"
                    | "not"
                    | "in"
                    | "is"
                    | "delete"
                    | "with"
                    | "as"
                    | "assert"
                    | "yield"
                    | "raise"
                    | "from"
                    | "global"
                    | "nonlocal"
                    | "lambda"
                    | "class"
                    | "def"
                    | "try"
                    | "finally"
                    | "match"
                    | "case"
                    | "else"
                    | "elif"
                    | "break"
                    | "continue"
            ) {
                continue;
            }
            violations.push(Violation::new(
                "BCS009",
                relative,
                line_number,
                found.get(0).map_or(0, |capture| capture.start()) + 1,
                "function name and opening parenthesis must not be separated",
                found.get(0).map_or("", |capture| capture.as_str()),
                "name(",
                false,
            ));
        }
        if let Some(found) = inner.find(line) {
            violations.push(Violation::new(
                "BCS009",
                relative,
                line_number,
                found.start() + 1,
                "parentheses must not contain padding spaces",
                found.as_str(),
                "no_padding",
                false,
            ));
        }
        let member_access = line.find(" . ").or_else(|| {
            if language.is_c_like() || language.is_script() {
                line.find(" -> ")
            } else {
                None
            }
        });
        if let Some(found) = member_access {
            violations.push(Violation::new(
                "BCS009",
                relative,
                line_number,
                found + 1,
                "member access operators must not have surrounding spaces",
                "spaced_member_access",
                "object.member",
                false,
            ));
        }
        line_start = line_end;
    }
}

fn ranges_overlap(ranges: &[Range<usize>], start: usize, end: usize) -> bool {
    let index = ranges.partition_point(|range| range.start < end);
    index > 0 && ranges[..index].iter().any(|range| range.end > start)
}

fn mask_ranges(
    source: &str,
    strings: &[Range<usize>],
    comments: &[Range<usize>],
    preserve: &[Range<usize>],
) -> String {
    let mut bytes = source.as_bytes().to_vec();
    for range in strings {
        for (index, byte) in bytes
            .get_mut(range.clone())
            .unwrap_or(&mut [])
            .iter_mut()
            .enumerate()
        {
            let offset = range.start + index;
            if *byte != b'\n'
                && !preserve
                    .iter()
                    .any(|preserve| preserve.start <= offset && offset < preserve.end)
            {
                *byte = b'x';
            }
        }
    }
    for range in comments {
        for byte in bytes.get_mut(range.clone()).unwrap_or(&mut []) {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_owned())
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
    )
}

fn is_nesting_construct(_language: Language, kind: &str) -> bool {
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
    )
}

fn is_variable_declaration(language: Language, node: Node) -> bool {
    match language {
        Language::Python => node.kind() == "assignment",
        Language::Rust => matches!(
            node.kind(),
            "let_declaration" | "const_item" | "static_item"
        ),
        _ if language.is_script() => matches!(
            node.kind(),
            "lexical_declaration" | "variable_declaration" | "variable_declarator"
        ),
        _ => matches!(
            node.kind(),
            "declaration" | "init_declarator" | "preproc_def"
        ),
    }
}

fn is_statement_list(kind: &str) -> bool {
    matches!(
        kind,
        "compound_statement" | "statement_block" | "block" | "module" | "program" | "source_file"
    )
}

fn is_statement(kind: &str) -> bool {
    kind.ends_with("_statement")
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

fn is_block(node: Node) -> bool {
    matches!(
        node.kind(),
        "compound_statement" | "statement_block" | "block"
    )
}

fn meaningful_statements(node: Node) -> Vec<Node> {
    (0..node.named_child_count())
        .filter_map(|index| node.named_child(index as u32))
        .filter(|child| child.kind() != "comment")
        .collect()
}

fn is_terminating(node: Node) -> bool {
    matches!(
        node.kind(),
        "return_statement"
            | "throw_statement"
            | "return_expression"
            | "break_expression"
            | "continue_expression"
    )
}

fn ends_with_terminator(source: &str, node: Node) -> bool {
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

fn contains_assignment_condition(node: Node) -> bool {
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

fn has_function_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if is_function(ancestor.kind()) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

fn has_type_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(ancestor) = current {
        if matches!(
            ancestor.kind(),
            "class_definition" | "class_declaration" | "class_specifier" | "struct_specifier"
        ) {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

fn find_parameters(function: Node) -> Option<Node> {
    function
        .child_by_field_name("parameters")
        .or_else(|| {
            let declarator = find_named_child_kind(function, "function_declarator")?;
            declarator.child_by_field_name("parameters")
        })
        .or_else(|| find_named_child_kind(function, "closure_parameters"))
}

fn parameter_nodes(language: Language, parameters: Node) -> Vec<Node> {
    (0..parameters.named_child_count())
        .filter_map(|index| parameters.named_child(index as u32))
        .filter(|node| {
            if node.kind() == "comment" {
                return false;
            }
            if language == Language::Rust {
                !matches!(node.kind(), "self_parameter" | "self")
            } else if language == Language::Python {
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
                        | "positional_separator"
                        | "keyword_separator"
                ) && !matches!(node.kind(), "positional_separator" | "keyword_separator")
            } else {
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
        })
        .filter(|node| {
            !(language == Language::Python
                && matches!(node.kind(), "identifier")
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() != "parameters"))
        })
        .collect()
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
        let mut sibling = parameter.prev_sibling();
        while let Some(current) = sibling {
            if current.kind() == "list_splat_pattern" || node_text(source, current) == "*" {
                return false;
            }
            sibling = current.prev_sibling();
        }
    }
    true
}

fn is_double_negation(source: &str, node: Node) -> bool {
    let text = node_text(source, node).trim_start();
    let negated = match node.kind() {
        "not_operator" => text.starts_with("not "),
        "unary_expression" | "unary_operator" => text.starts_with('!'),
        _ => false,
    };
    if !negated {
        return false;
    }
    let mut inner = meaningful_statements(node).into_iter().next();
    while let Some(current) = inner {
        if current.kind() == "parenthesized_expression" {
            inner = meaningful_statements(current).into_iter().next();
            continue;
        }
        let inner_text = node_text(source, current).trim_start();
        return match current.kind() {
            "not_operator" => inner_text.starts_with("not "),
            "unary_expression" | "unary_operator" => inner_text.starts_with('!'),
            _ => false,
        };
    }
    false
}

fn is_boolean_parameter(language: Language, text: &str) -> bool {
    if text.trim_start().starts_with(['{', '[']) {
        return false;
    }
    match language {
        Language::Python => text.contains(": bool") || text.contains(":bool"),
        _ if language.is_script() => text.contains(": boolean") || text.contains(":boolean"),
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

fn is_framework_method_name(language: Language, name: &str) -> bool {
    language == Language::Python
        && matches!(
            name,
            "do_GET" | "do_POST" | "do_PUT" | "do_DELETE" | "do_PATCH" | "do_HEAD" | "do_OPTIONS"
        )
}

fn is_react_component(language: Language, function: Node, name: &str) -> bool {
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

fn is_constructor_name(function: Node, source: &str, name: &str) -> bool {
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

fn find_named_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut found = None;
    visit(node, &mut |child| {
        if found.is_none() && child.kind() == kind {
            found = Some(child);
        }
    });
    found
}

fn case_can_fallthrough(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    let has_body = text.lines().count() > 1;
    let terminates = ["break", "return", "continue", "goto", "throw"]
        .iter()
        .any(|word| contains_word(text, word));
    has_body && !terminates
}

fn has_fallthrough_marker(source: &str, node: Node) -> bool {
    let text = node_text(source, node);
    text.contains("fallthrough") || text.contains("fall through")
}

fn first_token(source: &str, node: Node, token: char) -> Option<tree_sitter::Point> {
    let text = node_text(source, node);
    text.find(token).map(|index| {
        let prefix = &text[..index];
        let row = node.start_position().row + prefix.bytes().filter(|byte| *byte == b'\n').count();
        let column = if let Some(last) = prefix.rfind('\n') {
            prefix[last + 1..].len()
        } else {
            node.start_position().column + prefix.len()
        };
        tree_sitter::Point { row, column }
    })
}

fn valid_function_name(language: Language, name: &str) -> bool {
    if language.is_script() {
        valid_lower_camel(name)
    } else {
        valid_lower_snake(name)
    }
}

fn valid_lower_camel(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^[a-z][A-Za-z0-9]*$").unwrap())
        .is_match(name)
}

fn valid_lower_snake(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^[a-z_][a-z0-9_]*$").unwrap())
        .is_match(name)
}

fn valid_upper_snake(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^[A-Z_][A-Z0-9_]*$").unwrap())
        .is_match(name)
}

fn valid_pascal_case(name: &str) -> bool {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"^_*[A-Z][A-Za-z0-9]*$").unwrap())
        .is_match(name)
}

fn count_identifier(source: &str, name: &str) -> usize {
    if name.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut offset = 0;
    while let Some(index) = source[offset..].find(name) {
        let start = offset + index;
        let end = start + name.len();
        let boundary_before = source[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        let boundary_after = source[end..]
            .chars()
            .next()
            .is_none_or(|character| !character.is_alphanumeric() && character != '_');
        if boundary_before && boundary_after {
            count += 1;
        }
        offset = end;
    }
    count
}

fn contains_word(text: &str, word: &str) -> bool {
    let Ok(regex) = Regex::new(&format!(r"\b{}\b", regex::escape(word))) else {
        return false;
    };
    regex.is_match(text)
}

pub(crate) fn node_text<'a>(source: &'a str, node: Node) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

fn push_function_contract(
    violations: &mut Vec<Violation>,
    relative: &str,
    node: Node,
    rule: &str,
    message: &str,
    actual: impl Into<serde_json::Value>,
    expected: impl Into<serde_json::Value>,
) {
    push_function(violations, relative, node, rule, message, actual, expected);
    if let Some(violation) = violations.last_mut() {
        violation.mark_high_confidence_violation();
    }
}

fn push_function(
    violations: &mut Vec<Violation>,
    relative: &str,
    node: Node,
    rule: &str,
    message: &str,
    actual: impl Into<serde_json::Value>,
    expected: impl Into<serde_json::Value>,
) {
    let mut violation = Violation::new(
        rule,
        relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        message,
        actual,
        expected,
        false,
    );
    violation.range = crate::model::Range {
        start: crate::model::Position::new(
            node.start_position().row + 1,
            node.start_position().column + 1,
        ),
        end: crate::model::Position::new(
            node.end_position().row + 1,
            node.end_position().column + 1,
        ),
    };
    violation.severity = "warning".to_owned();
    violation.confidence = "low".to_owned();
    violations.push(violation);
}

fn push(
    violations: &mut Vec<Violation>,
    relative: &str,
    node: Node,
    rule: &str,
    message: &str,
    actual: impl Into<serde_json::Value>,
    expected: impl Into<serde_json::Value>,
) {
    push_actual(violations, relative, node, rule, message, actual, expected);
}

fn push_actual(
    violations: &mut Vec<Violation>,
    relative: &str,
    node: Node,
    rule: &str,
    message: &str,
    actual: impl Into<serde_json::Value>,
    expected: impl Into<serde_json::Value>,
) {
    violations.push(Violation::new(
        rule,
        relative,
        node.start_position().row + 1,
        node.start_position().column + 1,
        message,
        actual,
        expected,
        false,
    ));
}

fn visit<'tree>(node: Node<'tree>, callback: &mut impl FnMut(Node<'tree>)) {
    callback(node);
    for index in 0..node.named_child_count() {
        if let Some(child) = node.named_child(index as u32) {
            visit(child, callback);
        }
    }
}

pub fn coverage(analysis: &SyntaxAnalysis) -> Coverage {
    Coverage {
        executed: analysis.executed.clone(),
        not_implemented: BTreeSet::new(),
        partial: analysis.partial.clone(),
        not_applicable: analysis.not_applicable.clone(),
    }
}

pub fn dependency_path(import: &ImportRecord) -> Option<String> {
    if import.type_only {
        return None;
    }
    if import.language == Language::Python {
        return python_dependency_path(import);
    }
    if import.specifier.starts_with("crate::") {
        return Some(format!(
            "src/{}",
            import
                .specifier
                .trim_start_matches("crate::")
                .replace("::", "/")
        ));
    }
    if import.specifier.starts_with("self::") {
        return Some(format!(
            "./{}",
            import
                .specifier
                .trim_start_matches("self::")
                .replace("::", "/")
        ));
    }
    if import.specifier.starts_with("super::") {
        return Some(format!(
            "../{}",
            import
                .specifier
                .trim_start_matches("super::")
                .replace("::", "/")
        ));
    }
    let specifier = import.specifier.trim();
    if specifier.starts_with('.') || specifier.ends_with(".h") {
        Some(specifier.to_owned())
    } else {
        None
    }
}

fn python_dependency_path(import: &ImportRecord) -> Option<String> {
    let specifier = import.specifier.trim();
    if specifier.is_empty() {
        return None;
    }
    let dots = specifier
        .chars()
        .take_while(|character| *character == '.')
        .count();
    if dots == 0 {
        return Some(format!("/{}", specifier.replace('.', "/")));
    }

    let prefix = "../".repeat(dots - 1);
    let module = &specifier[dots..];
    if !module.is_empty() {
        return Some(format!("{prefix}{}", module.replace('.', "/")));
    }
    match import.binding.as_deref() {
        Some(binding) if !binding.is_empty() && binding != "*" => {
            Some(format!("{prefix}{binding}"))
        }
        _ if prefix.is_empty() => Some(".".to_owned()),
        _ => Some(prefix.trim_end_matches('/').to_owned()),
    }
}

pub fn imported_symbols(imports: &[ImportRecord]) -> BTreeMap<String, usize> {
    let mut symbols = BTreeMap::new();
    for import in imports {
        if let Some(binding) = &import.binding {
            *symbols.entry(binding.clone()).or_default() += 1;
        }
    }
    symbols
}
