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
        }
    }

    pub fn is_c_like(self) -> bool {
        matches!(self, Self::C | Self::Cpp)
    }

    pub fn is_script(self) -> bool {
        matches!(self, Self::JavaScript | Self::TypeScript | Self::Tsx)
    }

    pub fn ts_language(self) -> TsLanguage {
        match self {
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    pub fn parse(self, source: &str) -> Result<Tree, String> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language())
            .map_err(|error| error.to_string())?;
        parser
            .parse(source, None)
            .ok_or_else(|| "parser returned no tree".to_owned())
    }
}
