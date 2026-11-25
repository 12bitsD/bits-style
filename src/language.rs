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
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
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
    let mut index = 0;
    while index + 7 < output.len() {
        if &output[index..index + 7] != b"import<" {
            index += 1;
            continue;
        }
        output[index + 6] = b' ';
        let mut cursor = index + 7;
        let mut depth = 1_usize;
        let mut quote = None;
        let mut escaped = false;
        while cursor < output.len() && depth > 0 {
            let character = output[cursor];
            if let Some(current_quote) = quote {
                if escaped {
                    escaped = false;
                } else if character == b'\\' {
                    escaped = true;
                } else if character == current_quote {
                    quote = None;
                }
            } else {
                match character {
                    b'\'' | b'"' | b'`' => quote = Some(character),
                    b'<' => depth += 1,
                    b'>' => depth -= 1,
                    _ => {}
                }
            }
            if character != b'\n' {
                output[cursor] = b' ';
            }
            cursor += 1;
        }
        index = cursor;
    }

    let mut index = 0;
    while index + 14 < output.len() {
        if &output[index..index + 14] != b"typeof import("
            || index == 0
            || output[index - 1] != b'<'
        {
            index += 1;
            continue;
        }
        output[index - 1] = b' ';
        let mut cursor = index - 1;
        let mut depth = 1_usize;
        let mut quote = None;
        let mut escaped = false;
        while cursor < output.len() && depth > 0 {
            let character = output[cursor];
            if let Some(current_quote) = quote {
                if escaped {
                    escaped = false;
                } else if character == b'\\' {
                    escaped = true;
                } else if character == current_quote {
                    quote = None;
                }
            } else {
                match character {
                    b'\'' | b'"' | b'`' => quote = Some(character),
                    b'<' => depth += 1,
                    b'>' => depth -= 1,
                    _ => {}
                }
            }
            if character != b'\n' {
                output[cursor] = b' ';
            }
            cursor += 1;
        }
        index = cursor;
    }

    let mut index = 0;
    while index + 8 < output.len() {
        if &output[index..index + 7] != b"import(" {
            index += 1;
            continue;
        }
        let mut close = index + 7;
        let mut quote = None;
        let mut escaped = false;
        while close < output.len() {
            let character = output[close];
            if let Some(current_quote) = quote {
                if escaped {
                    escaped = false;
                } else if character == b'\\' {
                    escaped = true;
                } else if character == current_quote {
                    quote = None;
                }
            } else if character == b')' {
                break;
            } else if matches!(character, b'\'' | b'"' | b'`') {
                quote = Some(character);
            }
            close += 1;
        }
        if close + 2 >= output.len()
            || output[close + 1] != b'.'
            || !output[close + 2].is_ascii_uppercase()
        {
            index = close + 1;
            continue;
        }
        let mut end = close + 3;
        while end < output.len()
            && (output[end].is_ascii_alphanumeric()
                || matches!(output[end], b'_' | b'$' | b'[' | b']'))
        {
            end += 1;
        }
        for (offset, byte) in output[index..end].iter_mut().enumerate() {
            *byte = match offset {
                0 => b'a',
                1 => b'n',
                2 => b'y',
                _ => b' ',
            };
        }
        index = end;
    }
    String::from_utf8(output).unwrap_or_else(|_| source.to_owned())
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
