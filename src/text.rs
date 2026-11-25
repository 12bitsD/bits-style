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
    let mut strings = strings.to_vec();
    strings.sort_unstable_by_key(|range| range.start);
    let mut violations = Vec::new();
    let mut offset = 0;
    for (line_number, line) in (1..).zip(source.split_inclusive('\n')) {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let leading_len = content.len() - content.trim_start_matches([' ', '\t']).len();
        let leading = &content[..leading_len];
        let mixed = leading.contains(' ') && leading.contains('\t');
        let wrong_profile = match language {
            Language::C | Language::Cpp => leading.contains(' '),
            _ => leading.contains('\t'),
        };
        if (mixed || wrong_profile) && !inside_any(&strings, offset) {
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
        if trimmed.len() != content.len() && !inside_any(&strings, offset + trimmed.len()) {
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
        if columns > 80 && !long_string_exception(&strings, offset, content) {
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
            let mut violation = Violation::new(
                "BCS014",
                relative,
                point.0,
                point.1,
                "commented-out code must be deleted",
                text.lines().next().unwrap_or("").trim(),
                "no_commented_code",
                false,
            );
            violation.severity = "warning".to_owned();
            violations.push(violation);
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
    let mut strings = strings.to_vec();
    strings.sort_unstable_by_key(|range| range.start);
    let mut output = String::with_capacity(source.len() + 16);
    let mut offset = 0;
    let mut indentation_fixes = 0;
    let mut trailing_fixes = 0;

    for (line_number, line) in (1..).zip(source.split_inclusive('\n')) {
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
            continue;
        }
        if inside_any(&strings, offset) {
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
            if inside_any(&strings, trailing_start) {
                output.push_str(rest);
            } else {
                trailing_fixes += usize::from(trimmed.len() != rest.len());
                output.push_str(trimmed);
            }
        }
        output.push_str(ending);
        offset += line.len();
    }

    let mut newline_fixes = 0;
    let can_fix_newline = match line_scope {
        LineScope::WholeFile => true,
        LineScope::Added(lines) => lines.contains(&source.lines().count()),
    };
    if can_fix_newline && !output.is_empty() && !output.ends_with('\n') {
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
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && ranges[..index].iter().any(|range| range.end > offset)
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
    if body.starts_with("bits:allow")
        || body.starts_with("bits:explain")
        || body.starts_with("bits:prove")
    {
        return false;
    }
    static CODE: OnceLock<Regex> = OnceLock::new();
    let code = CODE.get_or_init(|| {
        Regex::new(r"^(?:return\s+.+|if\s*\(|for\s*\(|while\s*\(|switch\s*\(|(?:const|let|var)\s+[A-Za-z_]\w*\s*=|function\s+[A-Za-z_]\w*\s*\(|import\s+.+|export\s+.+|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*\s*=|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*\([^)]*\);?)$").unwrap()
    });
    body.lines().count() <= 3
        && body.lines().map(str::trim).any(|line| code.is_match(line))
        && !body.contains("TODO")
        && body.len() < 240
}

struct InlineAnnotation {
    rule: String,
    line: usize,
    payload: String,
    used: bool,
}

impl InlineAnnotation {
    fn matches(&self, violation: &Violation) -> bool {
        if self.rule != violation.rule_id {
            return false;
        }
        let start = violation.range.start.line;
        let end = violation.range.end.line;
        if violation.rule_id == "BCS004" {
            return (self.line >= start && self.line <= end)
                || (self.line < end && self.line + 1 >= start);
        }
        start == self.line || start == self.line + 1
    }
}

type InlineAnnotations = (Vec<InlineAnnotation>, Vec<InlineAnnotation>);

fn annotation_regexes() -> (&'static Regex, &'static Regex) {
    static SUPPRESSION: OnceLock<Regex> = OnceLock::new();
    static PROOF: OnceLock<Regex> = OnceLock::new();
    let suppression = SUPPRESSION
        .get_or_init(|| Regex::new(r"bits:allow\((BCS0(?:0[1-9]|1[0-7]))\):\s*(\S.*)").unwrap());
    let proof = PROOF.get_or_init(|| {
        Regex::new(concat!(
            r"bits:prove\((BCS0(?:0[1-9]|1[0-7]))",
            r"(?:,\s*test=([A-Za-z_]\w*(?:::\w+|\.\w+)*))?\):\s*",
            r"([^;；\r\n]+)[;；]\s*(\S[^\r\n]*)"
        ))
        .unwrap()
    });
    (suppression, proof)
}

fn annotation_error(
    relative: &str,
    line: usize,
    message: &str,
    actual: String,
    expected: &str,
) -> Violation {
    Violation::new(
        "BCS015", relative, line, 1, message, actual, expected, false,
    )
}

fn parse_allow(text: &str, line: usize, regex: &Regex) -> Result<InlineAnnotation, &'static str> {
    let captures = regex
        .captures(text)
        .ok_or("suppression must include a rule id and reason")?;
    Ok(InlineAnnotation {
        rule: captures[1].to_owned(),
        line,
        payload: String::new(),
        used: false,
    })
}

fn parse_prove(text: &str, line: usize, regex: &Regex) -> Result<InlineAnnotation, &'static str> {
    let captures = regex
        .captures(text)
        .ok_or("proof must state minimal form and concrete risk in two parts")?;
    let minimal = captures[3].trim();
    let risk = captures[4].trim();
    let payload = match captures.get(2) {
        Some(test) => format!("{minimal}; {risk} [test={}]", test.as_str()),
        None => format!("{minimal}; {risk}"),
    };
    Ok(InlineAnnotation {
        rule: captures[1].to_owned(),
        line,
        payload,
        used: false,
    })
}

fn parse_inline_annotations(
    relative: &str,
    source: &str,
    comments: &[Range<usize>],
    violations: &mut Vec<Violation>,
) -> InlineAnnotations {
    let (suppression, proof) = annotation_regexes();
    let mut allows = Vec::new();
    let mut proves = Vec::new();
    for comment in comments {
        let Some(text) = source.get(comment.clone()) else {
            continue;
        };
        let (line, _) = point_at(source, comment.start);
        let actual = || text.lines().next().unwrap_or("").to_owned();
        if text.contains("bits:allow") {
            match parse_allow(text, line, suppression) {
                Ok(annotation) => allows.push(annotation),
                Err(message) => violations.push(annotation_error(
                    relative,
                    line,
                    message,
                    actual(),
                    "bits:allow(BCSxxx): reason",
                )),
            }
        }
        if text.contains("bits:prove") {
            match parse_prove(text, line, proof) {
                Ok(annotation) => proves.push(annotation),
                Err(message) => violations.push(annotation_error(
                    relative,
                    line,
                    message,
                    actual(),
                    "bits:prove(BCSxxx[, test=name]): minimal reason; concrete risk",
                )),
            }
        }
    }
    (allows, proves)
}

fn apply_proofs(
    relative: &str,
    violations: &mut [Violation],
    proves: &mut [InlineAnnotation],
    extra: &mut Vec<Violation>,
) -> Vec<(usize, String)> {
    let mut matched = Vec::new();
    for violation in violations {
        let Some(annotation) = proves
            .iter_mut()
            .find(|annotation| !annotation.used && annotation.matches(violation))
        else {
            continue;
        };
        annotation.used = true;
        if violation.severity == "warning" {
            violation.explanation = Some(annotation.payload.clone());
            violation.obligation = Some("critic_review".to_owned());
            matched.push((annotation.line, annotation.rule.clone()));
        } else {
            extra.push(annotation_error(
                relative,
                annotation.line,
                "bits:prove cannot waive a hard violation",
                format!("bits:prove({})", annotation.rule),
                "refactor_or_external_contract_allow",
            ));
        }
    }
    matched
}

pub fn apply_inline_suppressions(
    relative: &str,
    source: &str,
    comments: &[Range<usize>],
    violations: &mut Vec<Violation>,
) -> Vec<(usize, String)> {
    let (mut allows, mut proves) = parse_inline_annotations(relative, source, comments, violations);
    violations.retain(|violation| {
        if let Some(annotation) = allows
            .iter_mut()
            .find(|annotation| annotation.matches(violation))
        {
            annotation.used = true;
            return false;
        }
        true
    });
    let mut extra = Vec::new();
    let matched = apply_proofs(relative, violations, &mut proves, &mut extra);
    for annotation in allows.into_iter().filter(|a| !a.used) {
        extra.push(annotation_error(
            relative,
            annotation.line,
            "unused bits:allow must be deleted",
            format!("bits:allow({})", annotation.rule),
            "used_suppression_or_delete",
        ));
    }
    for annotation in proves.into_iter().filter(|a| !a.used) {
        extra.push(annotation_error(
            relative,
            annotation.line,
            "unused bits:prove must be deleted",
            format!("bits:prove({})", annotation.rule),
            "active_warning_or_delete",
        ));
    }
    violations.extend(extra);
    matched
}

#[cfg(test)]
mod tests {
    use super::looks_like_commented_code;

    #[test]
    fn detects_only_statement_shaped_comments() {
        assert!(looks_like_commented_code("// return value;"));
        assert!(!looks_like_commented_code(
            "// 这里包含分号、括号和花括号，用于解释业务约束；不是旧实现。"
        ));
    }
}
