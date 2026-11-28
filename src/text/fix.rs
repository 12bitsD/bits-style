use std::ops::Range;

use crate::discovery::LineScope;
use crate::language::Language;
use crate::model::AppliedFix;
use crate::text::inside_any;

struct Fixer<'a> {
    language: Language,
    strings: Vec<Range<usize>>,
    line_scope: &'a LineScope,
}

pub fn fix(
    source: &str,
    strings: &[Range<usize>],
    language: Language,
    line_scope: &LineScope,
) -> (String, Vec<AppliedFix>) {
    let mut sorted = strings.to_vec();
    sorted.sort_unstable_by_key(|range| range.start);
    let fixer = Fixer {
        language,
        strings: sorted,
        line_scope,
    };
    fixer.run(source)
}

impl Fixer<'_> {
    fn run(&self, source: &str) -> (String, Vec<AppliedFix>) {
        let mut output = String::with_capacity(source.len() + 16);
        let mut offset = 0;
        let mut count = 0;
        for (line_number, line) in (1..).zip(source.split_inclusive('\n')) {
            let (content, ending) = split_line_ending(line);
            if line_in_scope(self.line_scope, line_number)
                && !inside_any(&self.strings, offset)
            {
                count += self.fix_line(offset, content, &mut output);
            } else {
                output.push_str(content);
            }
            output.push_str(ending);
            offset += line.len();
        }
        if line_in_scope(self.line_scope, source.lines().count())
            && !output.is_empty()
            && !output.ends_with('\n')
        {
            output.push('\n');
            count += 1;
        }
        let fixes = if count > 0 {
            vec![AppliedFix {
                rule_id: "BCS001".to_owned(),
                count,
            }]
        } else {
            Vec::new()
        };
        (output, fixes)
    }

    fn fix_line(
        &self,
        offset: usize,
        content: &str,
        output: &mut String,
    ) -> usize {
        let leading_len =
            content.len() - content.trim_start_matches([' ', '\t']).len();
        let leading = &content[..leading_len];
        let normalized = normalize_leading(self.language, leading);
        let indent_fix = usize::from(normalized != leading);
        output.push_str(&normalized);
        let rest = &content[leading_len..];
        let trimmed = rest.trim_end_matches([' ', '\t']);
        let trailing_start = offset + leading_len + trimmed.len();
        if inside_any(&self.strings, trailing_start) {
            output.push_str(rest);
            return indent_fix;
        }
        output.push_str(trimmed);
        indent_fix + usize::from(trimmed.len() != rest.len())
    }
}

fn split_line_ending(line: &str) -> (&str, &str) {
    let ending = if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    (&line[..line.len() - ending.len()], ending)
}

fn line_in_scope(scope: &LineScope, line_number: usize) -> bool {
    match scope {
        LineScope::WholeFile => true,
        LineScope::Added(lines) => lines.contains(&line_number),
    }
}

fn normalize_leading(language: Language, leading: &str) -> String {
    match language {
        Language::C | Language::Cpp => {
            let columns = leading.chars().fold(0_usize, |col, ch| {
                if ch == '\t' {
                    col + (8 - col % 8)
                } else {
                    col + 1
                }
            });
            "\t".repeat(columns.div_ceil(8))
        }
        Language::Python => leading.replace('\t', "    "),
        _ => leading.replace('\t', "  "),
    }
}
