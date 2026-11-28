use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

use crate::language::Language;
use crate::model::Violation;
use crate::text::point_at;

struct CommentScanner<'a> {
    language: Language,
    relative: &'a str,
    source: &'a str,
    violations: &'a mut Vec<Violation>,
}

pub(crate) fn scan_comments(
    input: &crate::text::TextInput<'_>,
    violations: &mut Vec<Violation>,
) {
    let mut scanner = CommentScanner {
        language: input.language,
        relative: input.relative,
        source: input.source,
        violations,
    };
    for comment in input.comments {
        scanner.scan_one(comment);
    }
}

impl CommentScanner<'_> {
    fn scan_one(&mut self, comment: &Range<usize>) {
        let Some(text) = self.source.get(comment.clone()) else {
            return;
        };
        let point = point_at(self.source, comment.start);
        self.check_todo(text, point);
        self.check_explanation(text, point);
        self.check_modeline(text, point);
        self.check_commented_code(comment, text, point);
    }

    fn check_todo(&mut self, text: &str, point: (usize, usize)) {
        let Some(index) = text.find("TODO") else {
            return;
        };
        let suffix = &text[index..];
        if !valid_todo(suffix) {
            self.violations.push(Violation::new(
                "BCS015",
                self.relative,
                point.0,
                point.1 + index,
                "TODO must include an owner and completion condition",
                suffix.lines().next().unwrap_or("TODO"),
                "TODO(owner): condition",
                false,
            ));
        }
    }

    fn check_explanation(&mut self, text: &str, point: (usize, usize)) {
        if text.contains("bits:explain") && !valid_explanation(text) {
            self.violations.push(Violation::new(
                "BCS015",
                self.relative,
                point.0,
                point.1,
                "explanation must use bits:explain(BCS003): reason",
                text.lines().next().unwrap_or(""),
                "bits:explain(BCS003): reason",
                false,
            ));
        }
    }

    fn check_modeline(&mut self, text: &str, point: (usize, usize)) {
        if text.contains("vim:set") || text.contains("-*- mode:") {
            self.violations.push(Violation::new(
                "BCS015",
                self.relative,
                point.0,
                point.1,
                "editor modelines are forbidden",
                "modeline",
                "no_modeline",
                false,
            ));
        }
    }

    fn check_commented_code(
        &mut self,
        comment: &Range<usize>,
        text: &str,
        point: (usize, usize),
    ) {
        let start =
            comment.start - text_start_offset(self.source, comment.start);
        let prefix = self.source[start..comment.start].trim_start();
        let go_pkg = self.language == Language::Go
            && prefix.starts_with("package ")
            && text.trim_start_matches("//").trim().starts_with("import ");
        if go_pkg
            || in_prose_comment_block(self.source, comment, self.language)
            || !looks_like_commented_code(self.language, self.relative, text)
        {
            return;
        }
        let mut violation = Violation::new(
            "BCS014",
            self.relative,
            point.0,
            point.1,
            "commented-out code must be deleted",
            text.lines().next().unwrap_or("").trim(),
            "no_commented_code",
            false,
        );
        violation.severity = "warning".to_owned();
        self.violations.push(violation);
    }
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

fn text_start_offset(source: &str, offset: usize) -> usize {
    source[..offset]
        .rfind('\n')
        .map_or(offset, |index| offset - index - 1)
}

fn comment_code_regex() -> &'static Regex {
    static CODE: OnceLock<Regex> = OnceLock::new();
    CODE.get_or_init(|| {
        Regex::new(r"^(?:return\s+.+|if\s*\(|for\s*\(|while\s*\(|switch\s*\(|(?:const|let|var)\s+[A-Za-z_]\w*\s*=\s*[^=\s].*|function\s+[A-Za-z_]\w*\s*\(|import\s+.+|export\s+.+|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*\s*=\s*[^=\s].*|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*\([^)]*\);?)$").unwrap()
    })
}

fn comment_body<'a>(raw: &'a str, marker: &str) -> &'a str {
    raw.trim().strip_prefix(marker).unwrap_or(raw).trim()
}

fn is_plain_line_comment(line: &str, marker: &str) -> bool {
    line.starts_with(marker)
        && !line.starts_with("///")
        && !line.starts_with("//!")
}

pub(crate) fn in_prose_comment_block(
    source: &str,
    comment: &Range<usize>,
    language: Language,
) -> bool {
    let Some(marker) = comment_marker(&source[comment.clone()], language)
    else {
        return false;
    };
    let block_start = rewind_comment_block(source, comment.start, marker);
    scan_comment_block_for_prose(source, block_start, marker)
}

fn comment_marker(raw: &str, language: Language) -> Option<&'static str> {
    if raw.starts_with("//") {
        Some("//")
    } else if raw.starts_with('#') && language == Language::Python {
        Some("#")
    } else {
        None
    }
}

fn rewind_comment_block(source: &str, start: usize, marker: &str) -> usize {
    let mut block_start = start - text_start_offset(source, start);
    while block_start > 0 {
        let prev_end = block_start - 1;
        let prev_start = prev_end - text_start_offset(source, prev_end);
        let prev = source[prev_start..prev_end].trim_start();
        if !is_plain_line_comment(prev, marker) {
            break;
        }
        block_start = prev_start;
    }
    block_start
}

fn scan_comment_block_for_prose(
    source: &str,
    block_start: usize,
    marker: &str,
) -> bool {
    let code = comment_code_regex();
    let mut cursor = block_start;
    let mut count = 0;
    let mut has_prose = false;
    while cursor < source.len() {
        let end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |idx| cursor + idx);
        let line = source[cursor..end].trim_start();
        if !is_plain_line_comment(line, marker) {
            break;
        }
        let body = comment_body(line, marker);
        count += 1;
        if body.is_empty()
            || ((!code.is_match(body) || is_comment_prose(body))
                && !body.starts_with("bits:"))
        {
            has_prose = true;
        }
        if end == source.len() {
            break;
        }
        cursor = end + 1;
    }
    count > 1 && has_prose
}

fn is_comment_prose(line: &str) -> bool {
    if is_return_prose(line) || line.contains(" => ") || line.contains(" or ") {
        return true;
    }
    if line.ends_with('.') && !line.ends_with("...") {
        return true;
    }
    line.strip_prefix("export ")
        .is_some_and(|r| r.starts_with("data ") || r.starts_with("the "))
}

pub(crate) fn looks_like_commented_code(
    language: Language,
    relative: &str,
    text: &str,
) -> bool {
    let trimmed = text.trim();
    if is_doc_comment(language, relative, trimmed) {
        return false;
    }
    let body = strip_comment_delimiters(trimmed);
    if is_bits_or_heading(language, body) || body.contains("TODO") {
        return false;
    }
    let code = comment_code_regex();
    body.len() < 240
        && body.lines().count() <= 3
        && body
            .lines()
            .map(str::trim)
            .any(|line| code.is_match(line) && !is_comment_prose(line))
}

fn is_doc_comment(language: Language, relative: &str, trimmed: &str) -> bool {
    let rust_doc = language == Language::Rust
        && (trimmed.starts_with("///") || trimmed.starts_with("//!"));
    let go_doc = language == Language::Go
        && (relative.ends_with("/doc.go")
            || relative == "doc.go"
            || trimmed.starts_with("//\t"));
    rust_doc || go_doc
}

fn strip_comment_delimiters(trimmed: &str) -> &str {
    trimmed
        .trim_start_matches("//")
        .trim_start_matches('#')
        .trim_start_matches("/*")
        .trim_end_matches("*/")
        .trim()
}

fn is_bits_or_heading(language: Language, body: &str) -> bool {
    body.starts_with("bits:allow")
        || body.starts_with("bits:explain")
        || body.starts_with("bits:prove")
        || (language == Language::Go && is_go_method_heading(body))
}

fn is_go_method_heading(body: &str) -> bool {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    HEADING
        .get_or_init(|| {
            Regex::new(r"^[A-Z]\w*(?:\.[A-Za-z_]\w*)+\(\)$").unwrap()
        })
        .is_match(body)
}

fn is_return_prose(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("return ") else {
        return false;
    };
    let english = [
        "the ", "a ", "an ", "whether ", "all ", "is ", "from ", "values ",
        "only ",
    ];
    if rest.ends_with('.') || english.iter().any(|p| rest.starts_with(p)) {
        return true;
    }
    let literals = [
        "true ", "false ", "nil ", "null ", "True ", "False ", "None ",
    ];
    !rest.ends_with(';') && literals.iter().any(|p| rest.starts_with(p))
}
