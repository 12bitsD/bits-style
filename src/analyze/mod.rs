mod ast;
mod branches;
mod c_checks;
mod control_flow;
mod dead_code;
mod dispatch;
mod env_checks;
mod errors;
mod expressions;
mod function_params;
mod functions;
mod go_checks;
mod import_parse;
mod imports;
mod lines;
mod metrics;
mod naming;
mod rust_imports;
mod spacing;
mod text_util;
mod types;
mod variables;

use std::collections::BTreeSet;

use tree_sitter::{Node, Tree};

use crate::language::Language;
use crate::model::Coverage;

use c_checks::analyze_inline;
use dead_code::analyze_unused_private_symbols;
use dispatch::dispatch_node;
use env_checks::is_type_alias;
use imports::analyze_import;
use lines::analyze_file_size;
use spacing::analyze_spacing;
use types::Analyzer;

pub(crate) use ast::{
    find_parameters, function_name, is_function, meaningful_statements,
    node_text, parameter_name, visit,
};
pub use dead_code::{dependency_path, imported_symbols};
pub(crate) use imports::python_reexports;
pub(crate) use metrics::function_metrics;
pub(crate) use naming::{is_python_keyword_only, parameter_nodes};
pub(crate) use text_util::{count_identifier, mask_ranges};
pub use types::{FunctionShape, ImportRecord, SyntaxAnalysis};

pub fn analyze(
    language: Language,
    relative: &str,
    source: &str,
    tree: &Tree,
) -> SyntaxAnalysis {
    let mut analyzer = Analyzer::new(language, relative, source);
    let root = tree.root_node();
    collect_syntax_ranges(&mut analyzer, root);
    record_language_coverage(&mut analyzer.result, language);
    visit(root, &mut |node| dispatch_node(&mut analyzer, root, node));
    run_file_level_passes(&mut analyzer, root);
    adjust_warning_severities(&mut analyzer.result, language);
    analyzer.result
}

fn collect_syntax_ranges(analyzer: &mut Analyzer<'_>, root: Node) {
    visit(root, &mut |node| {
        let kind = node.kind();
        if kind.contains("string")
            || matches!(kind, "raw_string_literal" | "template_string")
        {
            analyzer.result.strings.push(node.byte_range());
        }
        if matches!(kind, "comment" | "line_comment" | "block_comment") {
            analyzer.result.comments.push(node.byte_range());
        }
        if kind.starts_with("jsx_") {
            analyzer.result.jsx.push(node.byte_range());
        }
        if is_type_range_node(kind) {
            analyzer.result.type_ranges.push(node.byte_range());
        }
        if matches!(kind, "interpolation" | "template_substitution") {
            analyzer.result.interpolations.push(node.byte_range());
        }
        collect_python_annotation_strings(analyzer, node);
    });
}

fn is_type_range_node(kind: &str) -> bool {
    matches!(
        kind,
        "type_alias_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "enum_specifier"
            | "struct_specifier"
    )
}

fn collect_python_annotation_strings(analyzer: &mut Analyzer<'_>, node: Node) {
    if analyzer.language != Language::Python {
        return;
    }
    let kind = node.kind();
    let is_annotation = kind == "type"
        || (kind == "assignment"
            && is_type_alias(analyzer.language, analyzer.source, node))
        || (kind == "call"
            && node.child_by_field_name("function").is_some_and(|func| {
                matches!(
                    node_text(analyzer.source, func),
                    "cast" | "typing.cast"
                )
            }));
    if is_annotation {
        visit(node, &mut |child| {
            if child.kind() == "string" {
                analyzer.result.annotation_strings.push(child.byte_range());
            }
        });
    }
}

fn record_language_coverage(result: &mut SyntaxAnalysis, language: Language) {
    result.executed.extend(
        [
            "BCS003", "BCS004", "BCS005", "BCS006", "BCS010", "BCS011",
            "BCS014", "BCS016",
        ]
        .map(str::to_owned),
    );
    result.partial.extend(
        [
            "BCS002", "BCS003", "BCS004", "BCS005", "BCS006", "BCS008",
            "BCS009", "BCS010", "BCS011", "BCS012", "BCS013", "BCS014",
            "BCS016",
        ]
        .map(str::to_owned),
    );
    let bcs012_target = if language.is_c_like() {
        &mut result.executed
    } else {
        &mut result.not_applicable
    };
    bcs012_target.insert("BCS012".to_owned());
    if language == Language::Python
        || language.is_script()
        || matches!(language, Language::Cpp | Language::Rust)
    {
        result.executed.insert("BCS013".to_owned());
    }
    let brace_target =
        if matches!(language, Language::Python | Language::Go | Language::Rust)
        {
            &mut result.not_applicable
        } else {
            &mut result.executed
        };
    brace_target.extend(["BCS007", "BCS008"].map(str::to_owned));
    if language == Language::Go {
        result.executed.remove("BCS009");
        result
            .not_applicable
            .extend(["BCS002", "BCS009"].map(str::to_owned));
    } else {
        result.executed.insert("BCS009".to_owned());
    }
}

fn run_file_level_passes(analyzer: &mut Analyzer<'_>, root: Node) {
    if analyzer.language.is_c_like() {
        analyze_inline(analyzer, root);
    }
    analyze_file_size(analyzer, root);
    analyze_unused_private_symbols(analyzer, root);
    analyzer
        .result
        .jsx
        .sort_unstable_by_key(|range| range.start);
    if analyzer.language != Language::Go {
        analyze_spacing(analyzer);
    }
}

fn adjust_warning_severities(result: &mut SyntaxAnalysis, language: Language) {
    for violation in result
        .violations
        .iter_mut()
        .filter(|v| matches!(v.rule_id.as_str(), "BCS009" | "BCS010"))
    {
        violation.severity = "warning".to_owned();
        if violation.rule_id == "BCS009" && language == Language::Tsx {
            violation.confidence = "low".to_owned();
        }
    }
}

pub fn function_shapes(
    _language: Language,
    source: &str,
    tree: &Tree,
) -> Vec<FunctionShape> {
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

pub fn analyze_imports(
    language: Language,
    source: &str,
    tree: &Tree,
) -> Vec<ImportRecord> {
    let mut analyzer = Analyzer::new(language, "", source);
    visit(tree.root_node(), &mut |node| {
        if matches!(
            node.kind(),
            "import_statement"
                | "import_from_statement"
                | "import_declaration"
                | "preproc_include"
                | "use_declaration"
        ) {
            analyze_import(&mut analyzer, node, false);
        }
    });
    analyzer.result.imports
}

pub fn coverage(analysis: &SyntaxAnalysis) -> Coverage {
    Coverage {
        executed: analysis.executed.clone(),
        not_implemented: BTreeSet::new(),
        partial: analysis.partial.clone(),
        not_applicable: analysis.not_applicable.clone(),
    }
}
