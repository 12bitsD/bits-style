use std::collections::{BTreeMap, BTreeSet};

use crate::analyze::{self, ImportRecord, SyntaxAnalysis};
use crate::design::{self, DesignInput};
use crate::discovery::DiscoveredFile;
use crate::model::{FileFix, ScanError};
use crate::taint::{self, FunctionFact};
use crate::{test_index, text};

use super::FileScan;
use super::pipeline::ScanEnv;
use super::resolve::{python_package_context, resolve_dependency};

pub(super) fn scan_file(file: &DiscoveredFile, env: &ScanEnv<'_>) -> FileScan {
    let mut scan = FileScan {
        python_package: python_package_context(file),
        relative: file.relative.clone(),
        line_scope: file.line_scope.clone(),
        ..FileScan::default()
    };
    let (source, tree) = match read_and_parse(file) {
        Ok(parsed) => parsed,
        Err(error) => {
            scan.errors.push(error);
            return scan;
        }
    };
    populate_file_scan(&mut scan, file, (&source, &tree), env);
    scan
}

fn read_and_parse(
    file: &DiscoveredFile,
) -> Result<(String, tree_sitter::Tree), ScanError> {
    let source =
        std::fs::read_to_string(&file.absolute).map_err(|error| ScanError {
            file: file.relative.clone(),
            error: "read_error".to_owned(),
            message: error.to_string(),
        })?;
    let tree = file.language.parse(&source).map_err(|error| ScanError {
        file: file.relative.clone(),
        error: "parser_error".to_owned(),
        message: error,
    })?;
    if tree.root_node().has_error() {
        let location = crate::language::first_error_position(&tree)
            .map(|(line, column)| format!(" at {line}:{column}"))
            .unwrap_or_default();
        return Err(ScanError {
            file: file.relative.clone(),
            error: "parse_error".to_owned(),
            message: format!(
                "{} syntax tree contains errors{location}",
                file.language.name()
            ),
        });
    }
    Ok((source, tree))
}

fn populate_file_scan(
    scan: &mut FileScan,
    file: &DiscoveredFile,
    parsed: (&str, &tree_sitter::Tree),
    env: &ScanEnv<'_>,
) {
    let (source, tree) = parsed;
    let analysis =
        analyze::analyze(file.language, &file.relative, source, tree);
    design::analyze_design_mechanics(
        &DesignInput {
            language: file.language,
            relative: &file.relative,
            source,
            tree,
            strings: &analysis.strings,
            comments: &analysis.comments,
            interpolations: &analysis.interpolations,
        },
        &mut scan.violations,
    );
    scan.violations.extend(text::scan(&text::TextInput {
        language: file.language,
        relative: &file.relative,
        source,
        strings: &analysis.strings,
        comments: &analysis.comments,
    }));
    scan.shapes = analyze::function_shapes(file.language, source, tree);
    scan.tests = test_index::collect_tests(file.language, source, tree);
    let comments = analysis.comments.clone();
    scan.legacy_explains = text::legacy_explanation_lines(source, &comments);
    merge_analysis(scan, analysis);
    finalize_file_scan(scan, file, (source, tree, &comments), env);
}

fn finalize_file_scan(
    scan: &mut FileScan,
    file: &DiscoveredFile,
    parsed: (&str, &tree_sitter::Tree, &[std::ops::Range<usize>]),
    env: &ScanEnv<'_>,
) {
    let (source, tree, comments) = parsed;
    scan.facts = taint::extract_facts(&taint::FactInput {
        language: file.language,
        relative: &file.relative,
        source,
        tree,
        imports: &scan.imports,
        policy: env.policy,
        file_is_pure: env.layers.is_pure_file(&file.relative),
    });
    let (proves, allows) = text::apply_inline_suppressions(
        &file.relative,
        source,
        comments,
        &mut scan.violations,
    );
    scan.proves = proves;
    scan.allows = allows;
}

fn merge_analysis(scan: &mut FileScan, analysis: SyntaxAnalysis) {
    scan.violations.extend(analysis.violations);
    scan.imports.extend(analysis.imports);
    scan.executed.extend(analysis.executed);
    scan.partial.extend(analysis.partial);
    scan.not_applicable.extend(analysis.not_applicable);
}

pub(super) fn scan_dependency_context(
    scans: &[FileScan],
    files: &[DiscoveredFile],
    env: &ScanEnv<'_>,
) -> Vec<FileScan> {
    let context: BTreeMap<&str, &DiscoveredFile> = files
        .iter()
        .map(|file| (file.relative.as_str(), file))
        .collect();
    let known: BTreeSet<&str> = scans
        .iter()
        .map(|scan| scan.relative.as_str())
        .chain(context.keys().copied())
        .collect();
    let mut visited: BTreeSet<String> =
        scans.iter().map(|scan| scan.relative.clone()).collect();
    let mut pending = seed_context_queue(scans, &context, &known);
    let mut output = Vec::new();
    while let Some(relative) = pending.pop() {
        if !visited.insert(relative.clone()) {
            continue;
        }
        let Some(file) = context.get(relative.as_str()) else {
            continue;
        };
        if let Some(entry) = scan_context_entry(file, env, &known, &mut pending)
        {
            output.push(entry);
        }
    }
    output
}

fn seed_context_queue(
    scans: &[FileScan],
    context: &BTreeMap<&str, &DiscoveredFile>,
    known: &BTreeSet<&str>,
) -> Vec<String> {
    let mut pending: Vec<String> = context
        .values()
        .filter(|file| test_index::is_test_file_path(&file.relative))
        .map(|file| file.relative.clone())
        .collect();
    for scan in scans {
        enqueue_imports(scan, known, &mut pending);
    }
    pending
}

fn enqueue_imports(
    scan: &FileScan,
    known: &BTreeSet<&str>,
    pending: &mut Vec<String>,
) {
    for import in &scan.imports {
        let Some(specifier) = analyze::dependency_path(import) else {
            continue;
        };
        if let Some(target) = resolve_dependency(
            &scan.relative,
            &specifier,
            scan.python_package.as_ref(),
            known,
        ) {
            pending.push(target.to_owned());
        }
    }
}

fn scan_context_entry(
    file: &DiscoveredFile,
    env: &ScanEnv<'_>,
    known: &BTreeSet<&str>,
    pending: &mut Vec<String>,
) -> Option<FileScan> {
    let (imports, facts, tests) = scan_context_file(file, env)?;
    let entry = FileScan {
        python_package: python_package_context(file),
        relative: file.relative.clone(),
        imports,
        facts,
        tests,
        ..FileScan::default()
    };
    enqueue_imports(&entry, known, pending);
    Some(entry)
}

type ContextScan = (Vec<ImportRecord>, Vec<FunctionFact>, Vec<String>);

fn scan_context_file(
    file: &DiscoveredFile,
    env: &ScanEnv<'_>,
) -> Option<ContextScan> {
    let source = std::fs::read_to_string(&file.absolute).ok()?;
    let tree = file.language.parse(&source).ok()?;
    if tree.root_node().has_error() {
        return None;
    }
    let imports = analyze::analyze_imports(file.language, &source, &tree);
    let facts = taint::extract_facts(&taint::FactInput {
        language: file.language,
        relative: &file.relative,
        source: &source,
        tree: &tree,
        imports: &imports,
        policy: env.policy,
        file_is_pure: env.layers.is_pure_file(&file.relative),
    });
    let tests = test_index::collect_tests(file.language, &source, &tree);
    Some((imports, facts, tests))
}

pub(super) fn apply_fixes(
    files: &[DiscoveredFile],
) -> Result<Vec<FileFix>, String> {
    let mut records = Vec::new();
    for file in files {
        let source = std::fs::read_to_string(&file.absolute)
            .map_err(|error| error.to_string())?;
        let tree = match file.language.parse(&source) {
            Ok(tree) if !tree.root_node().has_error() => tree,
            _ => continue,
        };
        let analysis =
            analyze::analyze(file.language, &file.relative, &source, &tree);
        let (fixed, applied_fixes) = text::fix(
            &source,
            &analysis.strings,
            file.language,
            &file.line_scope,
        );
        let changed = fixed != source;
        if changed {
            std::fs::write(&file.absolute, fixed)
                .map_err(|error| error.to_string())?;
        }
        records.push(FileFix {
            file: file.relative.clone(),
            fixed: changed,
            applied_fixes,
            remaining_violations: 0,
        });
    }
    Ok(records)
}
