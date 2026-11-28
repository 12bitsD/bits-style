use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::Node;

use crate::language::Language;
use crate::model::Violation;

use super::ast::node_text;
use super::import_parse::{ImportEntry, extract_import_entries};
use super::rust_imports::is_rust_allowed_wildcard;
use super::text_util::{contains_word, count_identifier, mask_ranges};
use super::types::{Analyzer, ImportRecord};

pub(crate) fn analyze_import(
    analyzer: &mut Analyzer<'_>,
    node: Node,
    diagnostics: bool,
) {
    let ctx = ImportCheck::new(analyzer, node, diagnostics);
    let entries = extract_import_entries(analyzer, node, diagnostics);
    for entry in entries {
        record_import(analyzer, &ctx, entry);
    }
}

struct ImportCheck {
    node_line: usize,
    node_col: usize,
    text: String,
    masked: String,
    reexports: BTreeSet<String>,
    type_checking_block: bool,
    rust_allowed_wildcard: bool,
    diagnostics: bool,
}

impl ImportCheck {
    fn new(analyzer: &Analyzer<'_>, node: Node, diagnostics: bool) -> Self {
        let text = node_text(analyzer.source, node).trim().to_owned();
        let masked = if diagnostics {
            mask_ranges(
                analyzer.source,
                &analyzer.result.strings,
                &analyzer.result.comments,
                &analyzer.result.interpolations,
            )
        } else {
            String::new()
        };
        let reexports = if diagnostics {
            python_reexports(analyzer.source)
        } else {
            BTreeSet::new()
        };
        let type_checking_block = analyzer.language == Language::Python
            && in_type_checking_block(analyzer.source, node);
        let rust_allowed_wildcard = analyzer.language == Language::Rust
            && is_rust_allowed_wildcard(
                analyzer.relative,
                analyzer.source,
                node,
                &text,
            );
        Self {
            node_line: node.start_position().row + 1,
            node_col: node.start_position().column + 1,
            text,
            masked,
            reexports,
            type_checking_block,
            rust_allowed_wildcard,
            diagnostics,
        }
    }
}

fn record_import(
    analyzer: &mut Analyzer<'_>,
    ctx: &ImportCheck,
    entry: ImportEntry,
) {
    let (specifier, binding, type_only) = entry;
    if ctx.diagnostics {
        check_import_diagnostics(analyzer, ctx, binding.as_deref());
    }
    analyzer.result.imports.push(ImportRecord {
        language: analyzer.language,
        specifier,
        binding,
        line: ctx.node_line,
        column: ctx.node_col,
        type_only: type_only || ctx.type_checking_block,
    });
}

fn check_import_diagnostics(
    analyzer: &mut Analyzer<'_>,
    ctx: &ImportCheck,
    binding: Option<&str>,
) {
    let init_reexport = analyzer.language == Language::Python
        && analyzer.relative.ends_with("__init__.py")
        && ctx.text.starts_with("from .");
    let is_wildcard = binding == Some("*") || ctx.text.contains("import *");
    if is_wildcard && !init_reexport && !ctx.rust_allowed_wildcard {
        analyzer.result.violations.push(Violation::new(
            "BCS014",
            analyzer.relative,
            ctx.node_line,
            ctx.node_col,
            "wildcard imports are forbidden",
            ctx.text.as_str(),
            "explicit_imports",
            false,
        ));
    }
    if is_unused_import(analyzer, ctx, binding)
        && let Some(name) = binding
    {
        analyzer.result.violations.push(Violation::new(
            "BCS014",
            analyzer.relative,
            ctx.node_line,
            ctx.node_col,
            "unused import",
            name,
            "used_import",
            false,
        ));
    }
}

fn is_unused_import(
    analyzer: &Analyzer<'_>,
    ctx: &ImportCheck,
    binding: Option<&str>,
) -> bool {
    if matches!(analyzer.language, Language::Rust | Language::Go) {
        return false;
    }
    let Some(name) = binding else {
        return false;
    };
    let noqa = analyzer.language == Language::Python
        && (ctx.text.contains("noqa: F401") || ctx.text.contains("noqa:F401"));
    let forward_ref = analyzer.language == Language::Python
        && python_forward_annotation_ref(
            analyzer.source,
            &analyzer.result.annotation_strings,
            name,
        );
    name != "*"
        && !noqa
        && !ctx.reexports.contains(name)
        && !is_likely_trait_name(name)
        && count_identifier(&ctx.masked, name) <= 1
        && !forward_ref
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

fn python_forward_annotation_ref(
    source: &str,
    strings: &[Range<usize>],
    name: &str,
) -> bool {
    strings.iter().any(|range| {
        source
            .get(range.clone())
            .is_some_and(|literal| contains_word(literal, name))
    })
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

pub(crate) fn python_reexports(source: &str) -> BTreeSet<String> {
    static REEXPORT: OnceLock<Regex> = OnceLock::new();
    let regex = REEXPORT.get_or_init(|| {
        Regex::new(r#"[\"']([A-Za-z_][A-Za-z0-9_]*)[\"']"#).unwrap()
    });
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
