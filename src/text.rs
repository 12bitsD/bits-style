use std::ops::Range;

use crate::language::Language;
use crate::model::{AppliedFix, Violation};

pub fn scan(
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
        if leading.contains(' ') && !inside_any(strings, offset) {
            violations.push(Violation::new(
                "BCS001",
                relative,
                line_number,
                1,
                "indentation must use tabs only",
                leading.replace('\t', "\\t"),
                "tabs,width=8",
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
) -> (String, Vec<AppliedFix>) {
    let mut output = String::with_capacity(source.len() + 16);
    let mut offset = 0;
    let mut indentation_fixes = 0;
    let mut trailing_fixes = 0;

    for line in source.split_inclusive('\n') {
        let ending = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        let content = &line[..line.len() - ending.len()];
        if inside_any(strings, offset) {
            output.push_str(content);
        } else {
            let leading_len = content.len() - content.trim_start_matches([' ', '\t']).len();
            let leading = &content[..leading_len];
            if leading.contains(' ') {
                let columns = leading.chars().fold(0_usize, |column, character| {
                    if character == '\t' {
                        column + (8 - column % 8)
                    } else {
                        column + 1
                    }
                });
                let space_width = if language == Language::Python { 4 } else { 8 };
                let tabs = columns.div_ceil(space_width);
                output.push_str(&"\t".repeat(tabs));
                indentation_fixes += 1;
            } else {
                output.push_str(leading);
            }
            let rest = &content[leading_len..];
            let trimmed = rest.trim_end_matches([' ', '\t']);
            trailing_fixes += usize::from(trimmed.len() != rest.len());
            output.push_str(trimmed);
        }
        output.push_str(ending);
        offset += line.len();
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

fn looks_like_commented_code(text: &str) -> bool {
    let body = text
        .trim()
        .trim_start_matches("//")
        .trim_start_matches('#')
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim();
    let code_tokens = [
        "return ", "if (", "if(", "for (", "while (", " = ", ";", "{", "}", "=>",
    ];
    body.lines().count() <= 3
        && code_tokens.iter().any(|token| body.contains(token))
        && !body.contains("TODO")
        && body.len() < 240
}
