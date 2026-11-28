use std::collections::BTreeSet;
use std::ops::Range;

use serde_json::Value;
use tree_sitter::Node;

use crate::language::Language;
use crate::model::{Position, Range as ModelRange, Violation};

#[derive(Debug, Clone)]
pub struct ImportRecord {
    pub language: Language,
    pub specifier: String,
    pub binding: Option<String>,
    pub line: usize,
    pub column: usize,
    pub type_only: bool,
}

#[derive(Debug, Default)]
pub struct SyntaxAnalysis {
    pub violations: Vec<Violation>,
    pub strings: Vec<Range<usize>>,
    pub comments: Vec<Range<usize>>,
    pub jsx: Vec<Range<usize>>,
    pub type_ranges: Vec<Range<usize>>,
    pub interpolations: Vec<Range<usize>>,
    pub imports: Vec<ImportRecord>,
    pub annotation_strings: Vec<Range<usize>>,
    pub executed: BTreeSet<String>,
    pub partial: BTreeSet<String>,
    pub not_applicable: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct FunctionShape {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub nesting: usize,
    pub complexity: usize,
    pub cognitive: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionMetrics {
    pub nesting: usize,
    pub cyclomatic: usize,
    pub cognitive: usize,
    pub nesting_hotspots: Vec<(String, usize, usize)>,
    pub complexity_hotspots: Vec<(String, usize, usize)>,
}

pub(crate) struct Diag<'a> {
    pub node: Node<'a>,
    pub rule: &'static str,
    pub message: &'static str,
    pub actual: Value,
    pub expected: Value,
}

impl<'a> Diag<'a> {
    pub fn new(
        node: Node<'a>,
        rule: &'static str,
        message: &'static str,
        values: (impl Into<Value>, impl Into<Value>),
    ) -> Self {
        Self {
            node,
            rule,
            message,
            actual: values.0.into(),
            expected: values.1.into(),
        }
    }
}

pub(crate) struct Analyzer<'a> {
    pub language: Language,
    pub relative: &'a str,
    pub source: &'a str,
    pub result: SyntaxAnalysis,
}

impl<'a> Analyzer<'a> {
    pub fn new(language: Language, relative: &'a str, source: &'a str) -> Self {
        Self {
            language,
            relative,
            source,
            result: SyntaxAnalysis::default(),
        }
    }

    pub fn emit(&mut self, diag: Diag<'_>) -> &mut Violation {
        let pos = diag.node.start_position();
        self.result.violations.push(Violation::new(
            diag.rule,
            self.relative,
            pos.row + 1,
            pos.column + 1,
            diag.message,
            diag.actual,
            diag.expected,
            false,
        ));
        self.result.violations.last_mut().expect("just pushed")
    }

    pub fn emit_ranged(&mut self, diag: Diag<'_>) -> &mut Violation {
        let start = diag.node.start_position();
        let end = diag.node.end_position();
        let violation = self.emit(diag);
        violation.range = ModelRange {
            start: Position::new(start.row + 1, start.column + 1),
            end: Position::new(end.row + 1, end.column + 1),
        };
        violation
    }

    pub fn emit_fn_warning(&mut self, diag: Diag<'_>) -> &mut Violation {
        let violation = self.emit_ranged(diag);
        violation.severity = "warning".to_owned();
        violation.confidence = "low".to_owned();
        violation
    }

    pub fn emit_fn_contract(&mut self, diag: Diag<'_>) -> &mut Violation {
        let violation = self.emit_ranged(diag);
        violation.mark_high_confidence_violation();
        violation
    }

    pub fn emit_taste(
        &mut self,
        diag: Diag<'_>,
        category: &str,
    ) -> &mut Violation {
        let violation = self.emit(diag);
        violation.mark_taste_warning(category);
        violation
    }
}
