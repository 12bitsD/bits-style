use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

use crate::model::{AllowAnnotation, Violation};
use crate::text::point_at;

#[derive(Debug, Clone)]
pub struct MatchedProve {
    pub line: usize,
    pub rule: String,
    pub test: Option<String>,
}

struct InlineAnnotation {
    rule: String,
    line: usize,
    payload: String,
    test: Option<String>,
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
    let suppression = SUPPRESSION.get_or_init(|| {
        Regex::new(r"bits:allow\((BCS0(?:0[1-9]|1[0-7]))\):\s*(\S.*)").unwrap()
    });
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

#[derive(Clone, Copy)]
struct ErrorSite<'a> {
    relative: &'a str,
    line: usize,
}

fn annotation_error(
    site: ErrorSite<'_>,
    message: &str,
    actual: String,
    expected: &str,
) -> Violation {
    Violation::new(
        "BCS015",
        site.relative,
        site.line,
        1,
        message,
        actual,
        expected,
        false,
    )
}

fn parse_allow(
    text: &str,
    line: usize,
    regex: &Regex,
) -> Result<InlineAnnotation, &'static str> {
    let captures = regex
        .captures(text)
        .ok_or("suppression must include a rule id and reason")?;
    Ok(InlineAnnotation {
        rule: captures[1].to_owned(),
        line,
        payload: captures[2].trim().to_owned(),
        test: None,
        used: false,
    })
}

fn parse_prove(
    text: &str,
    line: usize,
    regex: &Regex,
) -> Result<InlineAnnotation, &'static str> {
    let captures = regex.captures(text).ok_or(
        "proof must state minimal form and concrete risk in two parts",
    )?;
    let minimal = captures[3].trim();
    let risk = captures[4].trim();
    let test = captures.get(2).map(|m| m.as_str().to_owned());
    let payload = match &test {
        Some(test) => format!("{minimal}; {risk} [test={test}]"),
        None => format!("{minimal}; {risk}"),
    };
    Ok(InlineAnnotation {
        rule: captures[1].to_owned(),
        line,
        payload,
        test,
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
        let site = ErrorSite { relative, line };
        if text.contains("bits:allow") {
            match parse_allow(text, line, suppression) {
                Ok(annotation) => allows.push(annotation),
                Err(message) => violations.push(annotation_error(
                    site,
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
                    site,
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
) -> Vec<MatchedProve> {
    let mut matched = Vec::new();
    for violation in violations {
        let Some(annotation) = proves.iter_mut().find(|annotation| {
            !annotation.used && annotation.matches(violation)
        }) else {
            continue;
        };
        annotation.used = true;
        if violation.severity == "warning" {
            violation.explanation = Some(annotation.payload.clone());
            violation.obligation = Some("critic_review".to_owned());
            matched.push(MatchedProve {
                line: annotation.line,
                rule: annotation.rule.clone(),
                test: annotation.test.clone(),
            });
        } else {
            let site = ErrorSite {
                relative,
                line: annotation.line,
            };
            extra.push(annotation_error(
                site,
                "bits:prove cannot waive a hard violation",
                format!("bits:prove({})", annotation.rule),
                "refactor_or_external_contract_allow",
            ));
        }
    }
    matched
}

pub fn legacy_explanation_lines(
    source: &str,
    comments: &[Range<usize>],
) -> Vec<usize> {
    comments
        .iter()
        .filter_map(|range| {
            let text = source.get(range.clone())?;
            text.contains("bits:explain")
                .then(|| point_at(source, range.start).0)
        })
        .collect()
}

pub fn apply_inline_suppressions(
    relative: &str,
    source: &str,
    comments: &[Range<usize>],
    violations: &mut Vec<Violation>,
) -> (Vec<MatchedProve>, Vec<AllowAnnotation>) {
    let (mut allows, mut proves) =
        parse_inline_annotations(relative, source, comments, violations);
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
    let used_allows = collect_used_allows(relative, allows, &mut extra);
    for annotation in proves.into_iter().filter(|a| !a.used) {
        let site = ErrorSite {
            relative,
            line: annotation.line,
        };
        extra.push(annotation_error(
            site,
            "unused bits:prove must be deleted",
            format!("bits:prove({})", annotation.rule),
            "active_warning_or_delete",
        ));
    }
    violations.extend(extra);
    (matched, used_allows)
}

fn collect_used_allows(
    relative: &str,
    allows: Vec<InlineAnnotation>,
    extra: &mut Vec<Violation>,
) -> Vec<AllowAnnotation> {
    let mut used = Vec::new();
    for annotation in allows {
        if annotation.used {
            used.push(AllowAnnotation {
                rule_id: annotation.rule,
                file: relative.to_owned(),
                line: annotation.line,
                reason: annotation.payload,
            });
        } else {
            let site = ErrorSite {
                relative,
                line: annotation.line,
            };
            extra.push(annotation_error(
                site,
                "unused bits:allow must be deleted",
                format!("bits:allow({})", annotation.rule),
                "used_suppression_or_delete",
            ));
        }
    }
    used
}
