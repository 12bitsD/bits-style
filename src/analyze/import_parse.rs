use tree_sitter::Node;

use crate::language::Language;

use super::ast::{node_text, visit};
use super::types::{Analyzer, Diag};

pub(super) type ImportEntry = (String, Option<String>, bool);

pub(super) fn extract_import_entries(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    diagnostics: bool,
) -> Vec<ImportEntry> {
    let text = node_text(analyzer.source, node).trim();
    match analyzer.language {
        Language::Rust => extract_rust_imports(text),
        Language::Go => extract_go_imports(analyzer, node, diagnostics),
        Language::Python => extract_python_imports(text),
        _ if analyzer.language.is_script() => extract_script_imports(text),
        _ => {
            let spec =
                text.split(['"', '<', '>']).nth(1).unwrap_or("").to_owned();
            vec![(spec, None, false)]
        }
    }
}

fn extract_rust_imports(text: &str) -> Vec<ImportEntry> {
    let is_reexport = text.starts_with("pub use ");
    let clause = text
        .trim_start_matches("pub use ")
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim();
    let binding = clause
        .rsplit("::")
        .next()
        .unwrap_or(clause)
        .trim_matches(|c| matches!(c, '{' | '}' | ' '));
    if binding == "*" || clause.contains("::*") {
        return vec![(clause.to_owned(), Some("*".to_owned()), false)];
    }
    if is_reexport {
        return Vec::new();
    }
    binding
        .split(',')
        .filter_map(|name| {
            let trimmed =
                name.trim().trim_matches(|c| matches!(c, '{' | '}' | ' '));
            let local = trimmed.split(" as ").last().unwrap_or(trimmed).trim();
            (!local.is_empty() && local != "self" && local != "_")
                .then(|| (clause.to_owned(), Some(local.to_owned()), false))
        })
        .collect()
}

fn extract_go_imports(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    diagnostics: bool,
) -> Vec<ImportEntry> {
    let mut entries = Vec::new();
    let mut dot_import = None;
    visit(node, &mut |child| {
        if child.kind() != "import_spec" {
            return;
        }
        let Some(path_node) = child.child_by_field_name("path") else {
            return;
        };
        let specifier = node_text(analyzer.source, path_node)
            .trim_matches(['"', '`'])
            .to_owned();
        let alias = child
            .child_by_field_name("name")
            .map(|name| node_text(analyzer.source, name).trim().to_owned());
        if alias.as_deref() == Some(".")
            && !analyzer.relative.ends_with("_test.go")
        {
            dot_import = Some(child);
        }
        let default_name =
            specifier.rsplit('/').next().unwrap_or("").to_owned();
        entries.push((specifier, Some(alias.unwrap_or(default_name)), false));
    });
    if diagnostics && let Some(dot) = dot_import {
        analyzer.emit_taste(
            Diag::new(
                dot,
                "BCS014",
                "dot imports obscure symbol origins",
                (node_text(analyzer.source, dot), "named_import"),
            ),
            "level-purity",
        );
    }
    entries
}

fn extract_python_imports(text: &str) -> Vec<ImportEntry> {
    if text.starts_with("from ") {
        let module = text.split_whitespace().nth(1).unwrap_or("");
        let imports = text.split_once(" import ").map_or("", |p| p.1);
        return imports
            .split(',')
            .filter_map(|name| {
                let cleaned =
                    name.trim().trim_matches(|c| matches!(c, '(' | ')' | '\\'));
                let binding =
                    cleaned.split(" as ").last().unwrap_or(cleaned).trim();
                (!binding.is_empty()).then(|| {
                    (module.to_owned(), Some(binding.to_owned()), false)
                })
            })
            .collect();
    }
    text.trim_start_matches("import ")
        .split(',')
        .map(|raw| {
            let name = raw.trim();
            let module = name.split(" as ").next().unwrap_or(name).trim();
            let binding = name.split(" as ").last().unwrap_or(name).trim();
            (module.to_owned(), Some(binding.to_owned()), false)
        })
        .collect()
}

fn extract_script_imports(text: &str) -> Vec<ImportEntry> {
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
        .map_or("", |parts| parts.0);
    let bindings = parse_js_bindings(clause);
    if bindings.is_empty() {
        vec![(specifier, None, false)]
    } else {
        bindings
            .into_iter()
            .map(|(binding, type_only)| {
                (specifier.clone(), Some(binding), type_only)
            })
            .collect()
    }
}

fn parse_js_bindings(clause: &str) -> Vec<(String, bool)> {
    let clause = clause.trim();
    let (clause, whole_type_only) = clause
        .strip_prefix("type ")
        .map_or((clause, false), |rest| (rest.trim(), true));
    let mut bindings = Vec::new();
    for part in split_top_level_commas(clause) {
        collect_js_part_bindings(part.trim(), whole_type_only, &mut bindings);
    }
    bindings
}

fn split_top_level_commas(clause: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut current = String::new();
    for ch in clause.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
}

fn collect_js_part_bindings(
    part: &str,
    whole_type_only: bool,
    bindings: &mut Vec<(String, bool)>,
) {
    if part.is_empty() {
        return;
    }
    if part.starts_with('*') {
        bindings.push(("*".to_owned(), whole_type_only));
        return;
    }
    for candidate in expand_js_braces(part) {
        let (item, item_type_only) = candidate
            .strip_prefix("type ")
            .map_or((candidate, false), |rest| (rest.trim(), true));
        let binding = item.split(" as ").last().unwrap_or(item).trim();
        if !binding.is_empty() {
            bindings
                .push((binding.to_owned(), whole_type_only || item_type_only));
        }
    }
}

fn expand_js_braces(part: &str) -> Vec<&str> {
    let Some((prefix, inside)) = part.split_once('{') else {
        return vec![part];
    };
    let mut out = Vec::new();
    if !prefix.trim().is_empty() {
        out.push(prefix.trim());
    }
    if let Some((body, _)) = inside.rsplit_once('}') {
        out.extend(body.split(',').map(str::trim));
    }
    out
}
