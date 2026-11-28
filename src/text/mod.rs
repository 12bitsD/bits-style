mod annotations;
mod comments;
mod fix;

use std::ops::Range;

use crate::language::Language;
use crate::model::Violation;

pub use annotations::{
    MatchedProve, apply_inline_suppressions, legacy_explanation_lines,
};
pub use fix::fix;

pub struct TextInput<'a> {
    pub language: Language,
    pub relative: &'a str,
    pub source: &'a str,
    pub strings: &'a [Range<usize>],
    pub comments: &'a [Range<usize>],
}

struct LineScanner<'a> {
    language: Language,
    relative: &'a str,
    strings: Vec<Range<usize>>,
    violations: Vec<Violation>,
}

pub fn scan(input: &TextInput<'_>) -> Vec<Violation> {
    let mut strings = input.strings.to_vec();
    strings.sort_unstable_by_key(|range| range.start);
    let mut scanner = LineScanner {
        language: input.language,
        relative: input.relative,
        strings,
        violations: Vec::new(),
    };
    scanner.scan_lines(input.source);
    scanner.check_eof_newline(input.source);
    comments::scan_comments(input, &mut scanner.violations);
    scanner.violations
}

#[derive(Clone, Copy)]
struct SourceLine<'a> {
    number: usize,
    offset: usize,
    content: &'a str,
}

impl LineScanner<'_> {
    fn scan_lines(&mut self, source: &str) {
        let mut offset = 0;
        for (number, line) in (1..).zip(source.split_inclusive('\n')) {
            let trimmed = line.strip_suffix('\n').unwrap_or(line);
            let content = trimmed.strip_suffix('\r').unwrap_or(trimmed);
            let item = SourceLine {
                number,
                offset,
                content,
            };
            self.check_indentation(item);
            self.check_trailing_space(item);
            self.check_line_width(item);
            offset += line.len();
        }
    }

    fn check_indentation(&mut self, line: SourceLine<'_>) {
        let content = line.content;
        let leading_len =
            content.len() - content.trim_start_matches([' ', '\t']).len();
        let leading = &content[..leading_len];
        let mixed = leading.contains(' ') && leading.contains('\t');
        let wrong_profile = match self.language {
            Language::C | Language::Cpp => leading.contains(' '),
            Language::Go => false,
            _ => leading.contains('\t'),
        };
        if !(mixed || wrong_profile) || inside_any(&self.strings, line.offset) {
            return;
        }
        let expected = match self.language {
            Language::C | Language::Cpp => "tabs,width=8",
            Language::Python => "spaces,width=4",
            Language::Go => "gofmt",
            _ => "spaces,width=2",
        };
        self.violations.push(Violation::new(
            "BCS001",
            self.relative,
            line.number,
            1,
            "indentation does not match the language profile",
            leading.replace('\t', "\\t"),
            expected,
            true,
        ));
    }

    fn check_trailing_space(&mut self, line: SourceLine<'_>) {
        let content = line.content;
        let trimmed = content.trim_end_matches([' ', '\t']);
        if trimmed.len() == content.len()
            || inside_any(&self.strings, line.offset + trimmed.len())
        {
            return;
        }
        self.violations.push(Violation::new(
            "BCS001",
            self.relative,
            line.number,
            trimmed.chars().count() + 1,
            "trailing whitespace is forbidden",
            content.len() - trimmed.len(),
            0,
            true,
        ));
    }

    fn check_line_width(&mut self, line: SourceLine<'_>) {
        let columns = line.content.chars().count();
        if self.language == Language::Go
            || columns <= 80
            || long_string_exception(&self.strings, line.offset, line.content)
            || url_line_exception(line.content)
        {
            return;
        }
        self.violations.push(Violation::new(
            "BCS002",
            self.relative,
            line.number,
            81,
            "line exceeds 80 columns",
            columns,
            "<=80",
            false,
        ));
    }

    fn check_eof_newline(&mut self, source: &str) {
        if source.is_empty() || source.ends_with('\n') {
            return;
        }
        let line = source.lines().count().max(1);
        let column = source.lines().last().unwrap_or("").chars().count() + 1;
        self.violations.push(Violation::new(
            "BCS001",
            self.relative,
            line,
            column,
            "file must end with one newline",
            "missing_newline",
            "newline",
            true,
        ));
    }
}

pub(crate) fn inside_any(ranges: &[Range<usize>], offset: usize) -> bool {
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && ranges[..index].iter().any(|range| range.end > offset)
}

fn long_string_exception(
    strings: &[Range<usize>],
    line_start: usize,
    content: &str,
) -> bool {
    let col80_byte = content
        .char_indices()
        .nth(79)
        .map_or(content.len(), |item| item.0);
    let col80_offset = line_start + col80_byte;
    let line_end = line_start + content.len();
    strings.iter().any(|range| {
        (range.start <= col80_offset && range.end > col80_offset)
            || (range.start <= line_start && range.end >= line_end)
    })
}

fn url_line_exception(content: &str) -> bool {
    content.contains("http://")
        || content.contains("https://")
        || content.contains("file://")
}

pub(crate) fn point_at(source: &str, offset: usize) -> (usize, usize) {
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

#[cfg(test)]
mod tests {
    use super::TextInput;
    use super::comments::{in_prose_comment_block, looks_like_commented_code};
    use crate::language::Language;

    #[test]
    fn detects_only_statement_shaped_comments() {
        assert!(looks_like_commented_code(
            Language::Python,
            "sample.py",
            "# return value"
        ));
        assert!(looks_like_commented_code(
            Language::Go,
            "sample.go",
            "// return value;"
        ));
        assert!(!looks_like_commented_code(
            Language::Python,
            "sample.py",
            "# 这里包含分号、括号和花括号，用于解释业务约束；不是旧实现。"
        ));
        assert!(!looks_like_commented_code(
            Language::Go,
            "command.go",
            "// return the args, excluding the one at this position."
        ));
        assert!(!looks_like_commented_code(
            Language::Go,
            "doc.go",
            "// import \"go.uber.org/zap\""
        ));
        assert!(!looks_like_commented_code(
            Language::Go,
            "buffer.go",
            "//\tsugar.Infof(\"hello\")"
        ));
        assert!(looks_like_commented_code(
            Language::JavaScript,
            "sample.js",
            "// const count = 1;"
        ));
        assert!(!looks_like_commented_code(
            Language::Go,
            "ssa.go",
            "// true => string iterator; false => map iterator."
        ));
        assert!(!looks_like_commented_code(
            Language::Go,
            "builder.go",
            "// length = len(x)."
        ));
    }

    #[test]
    fn prose_comment_blocks_and_package_imports_are_not_dead_code() {
        let design = "// loop:\n//     jump := READY\n//     x(y)\n//     return true\n//\n// where yield is synthetic:\n";
        let target = design.find("//     return true").unwrap();
        assert!(in_prose_comment_block(
            design,
            &(target..target + "//     return true".len()),
            Language::Go
        ));

        let dead = "// const a = 1;\n// const b = 2;\n// call(a, b);\n";
        let target = dead.find("// const b = 2;").unwrap();
        assert!(!in_prose_comment_block(
            dead,
            &(target..target + "// const b = 2;".len()),
            Language::JavaScript
        ));

        let package_doc =
            "package buffer // import \"go.uber.org/zap/buffer\"\n";
        let comment = 15..51;
        let violations = super::scan(&TextInput {
            language: Language::Go,
            relative: "buffer.go",
            source: package_doc,
            strings: &[],
            comments: std::slice::from_ref(&comment),
        });
        assert!(!violations.iter().any(|v| v.rule_id == "BCS014"));
    }
}
