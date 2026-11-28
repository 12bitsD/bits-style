use std::path::Path;

use tree_sitter::{Language as TsLanguage, Parser, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Language {
    C,
    Cpp,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Go,
}

impl Language {
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Some(Self::Cpp),
            "py" | "pyi" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "rs" => Some(Self::Rust),
            "go" => Some(Self::Go),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    pub fn is_c_like(self) -> bool {
        matches!(self, Self::C | Self::Cpp)
    }

    pub fn is_script(self) -> bool {
        matches!(self, Self::JavaScript | Self::TypeScript | Self::Tsx)
    }

    pub fn is_rust(self) -> bool {
        self == Self::Rust
    }

    pub fn is_go(self) -> bool {
        self == Self::Go
    }

    pub fn ts_language(self) -> TsLanguage {
        match self {
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => {
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
            }
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }

    pub fn parse(self, source: &str) -> Result<Tree, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language())
            .map_err(|error| error.to_string())?;
        let normalized = if self.is_script() {
            normalize_dynamic_import_generics(source)
        } else {
            source.to_owned()
        };
        parser
            .parse(&normalized, None)
            .ok_or_else(|| "parser returned no tree".to_owned())
    }
}

pub fn normalize_dynamic_import_generics(source: &str) -> String {
    let mut output = source.as_bytes().to_vec();
    blank_angle_generics(&mut output, b"import<", 6, 7);
    blank_typeof_import_generics(&mut output);
    replace_dynamic_import_types(&mut output);
    String::from_utf8(output).unwrap_or_else(|_| source.to_owned())
}

fn blank_angle_generics(
    output: &mut [u8],
    needle: &[u8],
    open_offset: usize,
    scan_offset: usize,
) {
    let mut index = 0;
    while index + needle.len() < output.len() {
        if &output[index..index + needle.len()] != needle {
            index += 1;
            continue;
        }
        output[index + open_offset] = b' ';
        index = blank_balanced_angles(output, index + scan_offset);
    }
}

fn blank_typeof_import_generics(output: &mut [u8]) {
    let needle = b"typeof import(";
    let mut index = 1;
    while index + needle.len() < output.len() {
        if &output[index..index + needle.len()] == needle
            && output[index - 1] == b'<'
        {
            output[index - 1] = b' ';
            index = blank_balanced_angles(output, index - 1);
            continue;
        }
        index += 1;
    }
}

fn blank_balanced_angles(output: &mut [u8], start: usize) -> usize {
    let mut cursor = start;
    let mut depth = 1_usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < output.len() && depth > 0 {
        let ch = output[cursor];
        step_quote_or_angle(ch, &mut quote, &mut escaped, &mut depth);
        if ch != b'\n' {
            output[cursor] = b' ';
        }
        cursor += 1;
    }
    cursor
}

fn step_quote_or_angle(
    ch: u8,
    quote: &mut Option<u8>,
    escaped: &mut bool,
    depth: &mut usize,
) {
    if let Some(active) = *quote {
        if *escaped {
            *escaped = false;
        } else if ch == b'\\' {
            *escaped = true;
        } else if ch == active {
            *quote = None;
        }
        return;
    }
    match ch {
        b'\'' | b'"' | b'`' => *quote = Some(ch),
        b'<' => *depth += 1,
        b'>' => *depth -= 1,
        _ => {}
    }
}

fn replace_dynamic_import_types(output: &mut [u8]) {
    let mut index = 0;
    while index + 8 < output.len() {
        if &output[index..index + 7] != b"import(" {
            index += 1;
            continue;
        }
        let close = find_closing_paren(output, index + 7);
        if !starts_dynamic_type_member(output, close) {
            index = close + 1;
            continue;
        }
        let end = scan_type_member_end(output, close + 3);
        overwrite_with_any(&mut output[index..end]);
        index = end;
    }
}

fn find_closing_paren(output: &[u8], start: usize) -> usize {
    let mut close = start;
    let mut quote = None;
    let mut escaped = false;
    while close < output.len() {
        let ch = output[close];
        if step_string_byte(ch, &mut quote, &mut escaped) {
            close += 1;
            continue;
        }
        if ch == b')' {
            break;
        }
        if matches!(ch, b'\'' | b'"' | b'`') {
            quote = Some(ch);
        }
        close += 1;
    }
    close
}

fn step_string_byte(
    ch: u8,
    quote: &mut Option<u8>,
    escaped: &mut bool,
) -> bool {
    let Some(active) = *quote else {
        return false;
    };
    if *escaped {
        *escaped = false;
    } else if ch == b'\\' {
        *escaped = true;
    } else if ch == active {
        *quote = None;
    }
    true
}

fn starts_dynamic_type_member(output: &[u8], close: usize) -> bool {
    close + 2 < output.len()
        && output[close + 1] == b'.'
        && output[close + 2].is_ascii_uppercase()
}

fn scan_type_member_end(output: &[u8], mut end: usize) -> usize {
    while end < output.len()
        && (output[end].is_ascii_alphanumeric()
            || matches!(output[end], b'_' | b'$' | b'[' | b']'))
    {
        end += 1;
    }
    end
}

fn overwrite_with_any(slice: &mut [u8]) {
    for (offset, byte) in slice.iter_mut().enumerate() {
        *byte = match offset {
            0 => b'a',
            1 => b'n',
            2 => b'y',
            _ => b' ',
        };
    }
}

pub fn first_error_position(tree: &Tree) -> Option<(usize, usize)> {
    fn visit(node: tree_sitter::Node) -> Option<(usize, usize)> {
        if node.kind() == "ERROR" || node.is_error() {
            return Some((
                node.start_position().row + 1,
                node.start_position().column + 1,
            ));
        }
        for index in 0..node.named_child_count() {
            if let Some(child) = node.named_child(index as u32)
                && let Some(position) = visit(child)
            {
                return Some(position);
            }
        }
        None
    }
    visit(tree.root_node())
}
