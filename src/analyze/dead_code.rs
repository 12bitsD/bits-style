use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

use crate::language::Language;

use super::ast::{function_name, node_text};
use super::imports::python_reexports;
use super::text_util::{count_identifier, mask_ranges};
use super::types::{Analyzer, Diag, ImportRecord};

pub(crate) fn analyze_unused_private_symbols(
    analyzer: &mut Analyzer<'_>,
    root: Node,
) {
    if !(analyzer.language == Language::Python || analyzer.language.is_script())
        || crate::test_index::is_test_file_path(analyzer.relative)
    {
        return;
    }
    if analyzer.language.is_script() && !has_export_statement(root) {
        return;
    }
    let masked = mask_ranges(
        analyzer.source,
        &analyzer.result.strings,
        &analyzer.result.comments,
        &analyzer.result.interpolations,
    );
    let candidates = collect_private_candidates(analyzer, root);
    for (name_node, name, message) in candidates {
        if count_identifier(&masked, &name) <= 1 {
            analyzer.emit_taste(
                Diag::new(
                    name_node,
                    "BCS014",
                    message,
                    (name, "used_or_deleted"),
                ),
                "state-gain",
            );
        }
    }
}

fn has_export_statement(root: Node) -> bool {
    (0..root.named_child_count()).any(|i| {
        root.named_child(i as u32)
            .is_some_and(|child| child.kind() == "export_statement")
    })
}

fn collect_private_candidates<'a>(
    analyzer: &Analyzer<'_>,
    root: Node<'a>,
) -> Vec<(Node<'a>, String, &'static str)> {
    let reexports = if analyzer.language == Language::Python {
        python_reexports(analyzer.source)
    } else {
        BTreeSet::new()
    };
    let mut candidates = Vec::new();
    for index in 0..root.named_child_count() {
        let Some(child) = root.named_child(index as u32) else {
            continue;
        };
        if analyzer.language == Language::Python {
            collect_python_private(
                analyzer.source,
                child,
                &reexports,
                &mut candidates,
            );
        } else if analyzer.language.is_script() {
            collect_script_private(analyzer.source, child, &mut candidates);
        }
    }
    candidates
}

fn collect_python_private<'a>(
    source: &str,
    child: Node<'a>,
    reexports: &BTreeSet<String>,
    out: &mut Vec<(Node<'a>, String, &'static str)>,
) {
    if child.kind() != "function_definition" {
        return;
    }
    let Some(name_node) = function_name(child) else {
        return;
    };
    let name = node_text(source, name_node);
    if name.starts_with('_')
        && !name.starts_with("__")
        && name != "_"
        && !reexports.contains(name)
    {
        out.push((name_node, name.to_owned(), "unused private function"));
    }
}

fn collect_script_private<'a>(
    source: &str,
    child: Node<'a>,
    out: &mut Vec<(Node<'a>, String, &'static str)>,
) {
    if matches!(
        child.kind(),
        "function_declaration" | "generator_function_declaration"
    ) {
        collect_script_fn(source, child, out);
        return;
    }
    if child.kind() == "lexical_declaration"
        && node_text(source, child).trim_start().starts_with("const ")
    {
        collect_script_const(source, child, out);
    }
}

fn collect_script_fn<'a>(
    source: &str,
    child: Node<'a>,
    out: &mut Vec<(Node<'a>, String, &'static str)>,
) {
    let Some(name_node) = function_name(child) else {
        return;
    };
    let name = node_text(source, name_node);
    if !name.is_empty() && name != "main" {
        out.push((name_node, name.to_owned(), "unused private function"));
    }
}

fn collect_script_const<'a>(
    source: &str,
    child: Node<'a>,
    out: &mut Vec<(Node<'a>, String, &'static str)>,
) {
    for index in 0..child.named_child_count() {
        let Some(decl) = child.named_child(index as u32) else {
            continue;
        };
        if decl.kind() != "variable_declarator" {
            continue;
        }
        if let Some(name_node) = decl.child_by_field_name("name")
            && name_node.kind() == "identifier"
        {
            let name = node_text(source, name_node);
            if !name.is_empty() && !name.starts_with('_') {
                out.push((name_node, name.to_owned(), "unused private symbol"));
            }
        }
    }
}

pub fn dependency_path(import: &ImportRecord) -> Option<String> {
    if import.type_only {
        return None;
    }
    if import.language == Language::Python {
        return python_dependency_path(import);
    }
    for (prefix, replacement) in
        [("crate::", "src/"), ("self::", "./"), ("super::", "../")]
    {
        if let Some(rest) = import.specifier.strip_prefix(prefix) {
            return Some(format!("{replacement}{}", rest.replace("::", "/")));
        }
    }
    let specifier = import.specifier.trim();
    (specifier.starts_with('.') || specifier.ends_with(".h"))
        .then(|| specifier.to_owned())
}

fn python_dependency_path(import: &ImportRecord) -> Option<String> {
    let specifier = import.specifier.trim();
    if specifier.is_empty() {
        return None;
    }
    let dots = specifier.chars().take_while(|c| *c == '.').count();
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
