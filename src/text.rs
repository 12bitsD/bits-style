use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

use crate::discovery::LineScope;
use crate::language::Language;
use crate::model::{AppliedFix, Violation};

pub fn scan(
    language: Language,
    relative: &str,
    source: &str,
    strings: &[Range<usize>],
    comments: &[Range<usize>],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let line_number = source[..offset]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let leading_len = content.len() - content.trim_start_matches([' ', '\t']).len();
        let leading = &content[..leading_len];
        let mixed = leading.contains(' ') && leading.contains('\t');
        let wrong_profile = match language {
            Language::C | Language::Cpp => leading.contains(' '),
            _ => leading.contains('\t'),
        };
        if (mixed || wrong_profile) && !inside_any(strings, offset) {
            let expected = match language {
                Language::C | Language::Cpp => "tabs,width=8",
                Language::Python => "spaces,width=4",
                _ => "spaces,width=2",
            };
            violations.push(Violation::new(
                "BCS001",
                relative,
                line_number,
                1,
                "indentation does not match the language profile",
                leading.replace('\t', "\\t"),
                expected,
                true,
            ));
        }
        let trimmed = content.trim_end_matches([' ', '\t']);
        if trimmed.len() != content.len() && !inside_any(strings, offset + trimmed.len()) {
            violations.push(Violation::new(
                "BCS001",
                relative,
                line_number,
                trimmed.chars().count() + 1,
                "trailing whitespace is forbidden",
                content.len() - trimmed.len(),
                0,
                true,
            ));
        }
        let columns = content.chars().count();
        if columns > 80 && !long_string_exception(strings, offset, content) {
            violations.push(Violation::new(
                "BCS002",
                relative,
                line_number,
                81,
                "line exceeds 80 columns",
                columns,
                "<=80",
                false,
            ));
        }
        offset += line.len();
    }

    if !source.is_empty() && !source.ends_with('\n') {
        let line = source.lines().count().max(1);
        let column = source.lines().last().unwrap_or("").chars().count() + 1;
        violations.push(Violation::new(
            "BCS001",
            relative,
            line,
            column,
            "file must end with one newline",
            "missing_newline",
            "newline",
            true,
        ));
    }

    for comment in comments {
        let Some(text) = source.get(comment.clone()) else {
            continue;
        };
        let point = point_at(source, comment.start);
        if let Some(index) = text.find("TODO") {
            let suffix = &text[index..];
            if !valid_todo(suffix) {
                violations.push(Violation::new(
                    "BCS015",
                    relative,
                    point.0,
                    point.1 + index,
                    "TODO must include an owner and completion condition",
                    suffix.lines().next().unwrap_or("TODO"),
                    "TODO(owner): condition",
                    false,
                ));
            }
        }
        if text.contains("bits:explain") && !valid_explanation(text) {
            violations.push(Violation::new(
                "BCS015",
                relative,
                point.0,
                point.1,
                "explanation must use bits:explain(BCS003): reason",
                text.lines().next().unwrap_or(""),
                "bits:explain(BCS003): reason",
                false,
            ));
        }
        if text.contains("vim:set") || text.contains("-*- mode:") {
            violations.push(Violation::new(
                "BCS015",
                relative,
                point.0,
                point.1,
                "editor modelines are forbidden",
                "modeline",
                "no_modeline",
                false,
            ));
        }
        if looks_like_commented_code(text) {
            violations.push(Violation::new(
                "BCS014",
                relative,
                point.0,
                point.1,
                "commented-out code must be deleted",
                text.lines().next().unwrap_or("").trim(),
                "no_commented_code",
                false,
            ));
        }
    }
    violations
}

pub fn fix(
    source: &str,
    strings: &[Range<usize>],
    language: Language,
    line_scope: &LineScope,
) -> (String, Vec<AppliedFix>) {
    let mut output = String::with_capacity(source.len() + 16);
    let mut offset = 0;
    let mut indentation_fixes = 0;
    let mut trailing_fixes = 0;
    let mut line_number = 1;

    for line in source.split_inclusive('\n') {
        let ending = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let content = &line[..line.len() - ending.len()];
        let can_fix = match line_scope {
            LineScope::WholeFile => true,
            LineScope::Added(lines) => lines.contains(&line_number),
        };
        if !can_fix {
            output.push_str(content);
            output.push_str(ending);
            offset += line.len();
            line_number += 1;
            continue;
        }
        if inside_any(strings, offset) {
            output.push_str(content);
        } else {
            let leading_len = content.len() - content.trim_start_matches([' ', '\t']).len();
            let leading = &content[..leading_len];
            let normalized = match language {
                Language::C | Language::Cpp => {
                    let columns = leading.chars().fold(0_usize, |column, character| {
                        if character == '\t' {
                            column + (8 - column % 8)
                        } else {
                            column + 1
                        }
                    });
                    "\t".repeat(columns.div_ceil(8))
                }
                Language::Python => leading.replace('\t', "    "),
                _ => leading.replace('\t', "  "),
            };
            if normalized != leading {
                indentation_fixes += 1;
            }
            output.push_str(&normalized);
            let rest = &content[leading_len..];
            let trimmed = rest.trim_end_matches([' ', '\t']);
            let trailing_start = offset + leading_len + trimmed.len();
            if inside_any(strings, trailing_start) {
                output.push_str(rest);
            } else {
                trailing_fixes += usize::from(trimmed.len() != rest.len());
                output.push_str(trimmed);
            }
        }
        output.push_str(ending);
        offset += line.len();
        line_number += 1;
    }

    let mut newline_fixes = 0;
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
        newline_fixes = 1;
    }
    let mut fixes = Vec::new();
    let whitespace = indentation_fixes + trailing_fixes + newline_fixes;
    if whitespace > 0 {
        fixes.push(AppliedFix {
            rule_id: "BCS001".to_owned(),
            count: whitespace,
        });
    }
    (output, fixes)
}

fn inside_any(ranges: &[Range<usize>], offset: usize) -> bool {
    ranges
        .iter()
        .any(|range| range.start < offset && offset < range.end)
}

fn long_string_exception(strings: &[Range<usize>], line_start: usize, content: &str) -> bool {
    strings.iter().any(|range| {
        range.start >= line_start
            && range.start < line_start + content.len()
            && range.end > line_start + content.len().min(80)
            && range.start
                <= line_start
                    + content
                        .char_indices()
                        .nth(79)
                        .map_or(content.len(), |item| item.0)
    })
}

fn point_at(source: &str, offset: usize) -> (usize, usize) {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |suffix| suffix.1)
        .chars()
        .count()
        + 1;
    (line, column)
}

fn valid_todo(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("TODO(") else {
        return false;
    };
    let Some((owner, condition)) = rest.split_once("):") else {
        return false;
    };
    !owner.trim().is_empty()
        && condition
            .split('\n')
            .next()
            .is_some_and(|value| !value.trim().is_empty())
}

fn valid_explanation(text: &str) -> bool {
    static EXPLANATION: OnceLock<Regex> = OnceLock::new();
    EXPLANATION
        .get_or_init(|| Regex::new(r"bits:explain\(BCS003\):\s*\S.*").unwrap())
        .is_match(text)
}

fn looks_like_commented_code(text: &str) -> bool {
    let body = text
        .trim()
        .trim_start_matches("//")
        .trim_start_matches('#')
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim();
    if body.starts_with("bits:allow") || body.starts_with("bits:explain") {
        return false;
    }
    let code_tokens = [
        "return ", "if (", "if(", "for (", "while (", " = ", ";", "{", "}", "=>",
    ];
    body.lines().count() <= 3
        && code_tokens.iter().any(|token| body.contains(token))
        && !body.contains("TODO")
        && body.len() < 240
}

pub fn apply_inline_suppressions(
    relative: &str,
    source: &str,
    comments: &[Range<usize>],
    violations: &mut Vec<Violation>,
) {
    static SUPPRESSION: OnceLock<Regex> = OnceLock::new();
    let suppression = SUPPRESSION
        .get_or_init(|| Regex::new(r"bits:allow\((BCS0(?:0[1-9]|1[0-5]))\):\s*(\S.*)").unwrap());
    let mut allowed = BTreeSet::new();
    for comment in comments {
        let Some(text) = source.get(comment.clone()) else {
            continue;
        };
        if !text.contains("bits:allow") {
            continue;
        }
        let (line, _) = point_at(source, comment.start);
        if let Some(captures) = suppression.captures(text) {
            let rule = captures[1].to_owned();
            allowed.insert((rule.clone(), line));
            allowed.insert((rule, line + 1));
        } else {
            violations.push(Violation::new(
                "BCS015",
                relative,
                line,
                1,
                "suppression must include a rule id and reason",
                text.lines().next().unwrap_or(""),
                "bits:allow(BCSxxx): reason",
                false,
            ));
        }
    }
    violations.retain(|violation| {
        !allowed.contains(&(violation.rule_id.clone(), violation.range.start.line))
    });
}
