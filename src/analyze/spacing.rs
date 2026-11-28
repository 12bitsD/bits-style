use std::sync::OnceLock;

use regex::Regex;
use tree_sitter::Node;

use crate::model::Violation;

use super::ast::node_text;
use super::text_util::{mask_ranges, ranges_overlap};
use super::types::{Analyzer, Diag};

const SPACED_KEYWORDS: &[&str] = &[
    "if", "switch", "for", "while", "catch", "return", "sizeof", "typeof",
    "alignof", "async", "await", "function", "new", "import", "export", "and",
    "or", "except", "not", "in", "is", "delete", "with", "as", "assert",
    "yield", "raise", "from", "global", "nonlocal", "lambda", "class", "def",
    "try", "finally", "match", "case", "else", "elif", "break", "continue",
    "let", "mut", "ref", "type", "pub", "where", "fn", "move", "const",
    "static", "unsafe", "loop", "extern",
];

pub(crate) fn analyze_binary_operator(analyzer: &mut Analyzer<'_>, node: Node) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let Some(right) = node.child_by_field_name("right") else {
        return;
    };
    if left.end_position().row != right.start_position().row {
        return;
    }
    let gap = &analyzer.source[left.end_byte()..right.start_byte()];
    let operator = node
        .child_by_field_name("operator")
        .map(|operator| node_text(analyzer.source, operator))
        .unwrap_or_else(|| gap.trim());
    if operator.is_empty() {
        return;
    }
    let expected = format!(" {operator} ");
    if gap != expected {
        analyzer.emit(Diag::new(
            node,
            "BCS009",
            "binary operators must have one space on each side",
            (gap, expected),
        ));
    }
}

pub(crate) fn analyze_spacing(analyzer: &mut Analyzer<'_>) {
    let masked = mask_ranges(
        analyzer.source,
        &analyzer.result.strings,
        &analyzer.result.comments,
        &[],
    );
    let jsx = analyzer.result.jsx.clone();
    let mut line_start = 0;
    for (index, line) in masked.lines().enumerate() {
        let line_number = index + 1;
        let line_end = line_start + line.len() + 1;
        if !ranges_overlap(&jsx, line_start, line_end) {
            check_spacing_line(analyzer, line, line_number);
        }
        line_start = line_end;
    }
}

fn check_spacing_line(
    analyzer: &mut Analyzer<'_>,
    line: &str,
    line_number: usize,
) {
    check_keyword_spacing(analyzer, line, line_number);
    check_call_spacing(analyzer, line, line_number);
    check_paren_padding(analyzer, line, line_number);
    check_member_access_spacing(analyzer, line, line_number);
}

fn check_keyword_spacing(
    analyzer: &mut Analyzer<'_>,
    line: &str,
    line_number: usize,
) {
    static KEYWORD: OnceLock<Regex> = OnceLock::new();
    let regex = KEYWORD.get_or_init(|| {
        Regex::new(r"\b(if|switch|for|while|catch)\(").unwrap()
    });
    let Some(found) = regex.find(line) else {
        return;
    };
    if found.start() > 0 && line[..found.start()].ends_with('.') {
        return;
    }
    analyzer.result.violations.push(Violation::new(
        "BCS009",
        analyzer.relative,
        line_number,
        found.start() + 1,
        "control keyword must be followed by one space",
        &line[found.range()],
        "keyword (",
        false,
    ));
}

fn check_call_spacing(
    analyzer: &mut Analyzer<'_>,
    line: &str,
    line_number: usize,
) {
    static CALL: OnceLock<Regex> = OnceLock::new();
    let regex = CALL.get_or_init(|| {
        Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)[ \t]+\(").unwrap()
    });
    for found in regex.captures_iter(line) {
        let name = found.get(1).map_or("", |c| c.as_str());
        if SPACED_KEYWORDS.contains(&name) {
            continue;
        }
        analyzer.result.violations.push(Violation::new(
            "BCS009",
            analyzer.relative,
            line_number,
            found.get(0).map_or(0, |c| c.start()) + 1,
            "function name and opening parenthesis must not be separated",
            found.get(0).map_or("", |c| c.as_str()),
            "name(",
            false,
        ));
    }
}

fn check_paren_padding(
    analyzer: &mut Analyzer<'_>,
    line: &str,
    line_number: usize,
) {
    static INNER: OnceLock<Regex> = OnceLock::new();
    let regex =
        INNER.get_or_init(|| Regex::new(r"\([ \t]+[^()\n]*[ \t]+\)").unwrap());
    if let Some(found) = regex.find(line) {
        analyzer.result.violations.push(Violation::new(
            "BCS009",
            analyzer.relative,
            line_number,
            found.start() + 1,
            "parentheses must not contain padding spaces",
            found.as_str(),
            "no_padding",
            false,
        ));
    }
}

fn check_member_access_spacing(
    analyzer: &mut Analyzer<'_>,
    line: &str,
    line_number: usize,
) {
    let member_access = line.find(" . ").or_else(|| {
        (analyzer.language.is_c_like() || analyzer.language.is_script())
            .then(|| line.find(" -> "))
            .flatten()
    });
    if let Some(found) = member_access {
        analyzer.result.violations.push(Violation::new(
            "BCS009",
            analyzer.relative,
            line_number,
            found + 1,
            "member access operators must not have surrounding spaces",
            "spaced_member_access",
            "object.member",
            false,
        ));
    }
}
