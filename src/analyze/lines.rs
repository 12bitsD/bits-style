use std::ops::Range;

use tree_sitter::Node;

use crate::model::Violation;

use super::text_util::inside_sorted_ranges;
use super::types::Analyzer;

pub(crate) fn code_line_count(
    analyzer: &Analyzer<'_>,
    function: Node,
) -> usize {
    let mut ignored = analyzer.result.comments.clone();
    ignored.extend_from_slice(&analyzer.result.strings);
    ignored.extend_from_slice(&analyzer.result.type_ranges);
    ignored.sort_unstable_by_key(|range| range.start);
    count_span_lines(analyzer.source, function, &ignored)
}

fn count_span_lines(
    source: &str,
    node: Node,
    ignored: &[Range<usize>],
) -> usize {
    let mut lines = 0;
    let bytes = source.as_bytes();
    let mut line_start = node.start_byte();
    let last_row = node.end_position().row;
    for row in node.start_position().row..=last_row {
        let line_end = if row == last_row {
            node.end_byte()
        } else {
            source[line_start..]
                .find('\n')
                .map_or(node.end_byte(), |index| line_start + index + 1)
        };
        if line_has_code(bytes, line_start, line_end, ignored) {
            lines += 1;
        }
        line_start = line_end;
    }
    lines
}

fn line_has_code(
    bytes: &[u8],
    line_start: usize,
    line_end: usize,
    ignored: &[Range<usize>],
) -> bool {
    bytes[line_start..line_end]
        .iter()
        .enumerate()
        .any(|(i, byte)| {
            !byte.is_ascii_whitespace()
                && !inside_sorted_ranges(ignored, line_start + i)
        })
}

pub(crate) fn analyze_file_size(analyzer: &mut Analyzer<'_>, root: Node) {
    if crate::test_index::is_test_file_path(analyzer.relative) {
        return;
    }
    let ignored = file_ignored_ranges(analyzer, root);
    let lines = count_file_lines(analyzer.source, &ignored);
    if lines > 300 {
        let mut violation = Violation::new(
            "BCS003",
            analyzer.relative,
            1,
            1,
            "file exceeds 300 code lines",
            lines,
            "<=300",
            false,
        );
        violation.mark_taste_warning("level-purity");
        analyzer.result.violations.push(violation);
    }
}

fn file_ignored_ranges(
    analyzer: &Analyzer<'_>,
    root: Node,
) -> Vec<Range<usize>> {
    let mut ignored = analyzer.result.comments.clone();
    ignored.extend_from_slice(&analyzer.result.strings);
    ignored.extend_from_slice(&analyzer.result.type_ranges);
    for index in 0..root.named_child_count() {
        if let Some(child) = root.named_child(index as u32)
            && matches!(
                child.kind(),
                "import_statement"
                    | "import_from_statement"
                    | "import_declaration"
                    | "preproc_include"
                    | "use_declaration"
                    | "package_clause"
            )
        {
            ignored.push(child.byte_range());
        }
    }
    ignored.sort_unstable_by_key(|range| range.start);
    ignored
}

fn count_file_lines(source: &str, ignored: &[Range<usize>]) -> usize {
    let mut lines = 0;
    let bytes = source.as_bytes();
    let mut line_start = 0;
    for line in source.split_inclusive('\n') {
        let line_end = line_start + line.len();
        if line_has_code(bytes, line_start, line_end, ignored) {
            lines += 1;
        }
        line_start = line_end;
    }
    lines
}
