use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: &str = "bits.style.scan/v1";
pub const STYLE_VERSION: &str = "0.2";
pub const RULE_IDS: [&str; 15] = [
    "BCS001", "BCS002", "BCS003", "BCS004", "BCS005", "BCS006", "BCS007", "BCS008", "BCS009",
    "BCS010", "BCS011", "BCS012", "BCS013", "BCS014", "BCS015",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(line: usize, column: usize) -> Self {
        Self {
            start: Position::new(line, column),
            end: Position::new(line, column + 1),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub rule_id: String,
    pub file: String,
    pub range: Range,
    pub message: String,
    pub actual: Value,
    pub expected: Value,
    pub fixable: bool,
}

impl Violation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rule_id: &str,
        file: &str,
        line: usize,
        column: usize,
        message: impl Into<String>,
        actual: impl Into<Value>,
        expected: impl Into<Value>,
        fixable: bool,
    ) -> Self {
        Self {
            rule_id: rule_id.to_owned(),
            file: file.to_owned(),
            range: Range::new(line, column),
            message: message.into(),
            actual: actual.into(),
            expected: expected.into(),
            fixable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanError {
    pub file: String,
    pub error: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    pub executed: BTreeSet<String>,
    pub not_implemented: BTreeSet<String>,
    pub partial: BTreeSet<String>,
    pub not_applicable: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub files_discovered: usize,
    pub files_scanned: usize,
    pub violations: usize,
    pub fixable_violations: usize,
    pub scan_errors: usize,
    pub files_fixed: usize,
    pub violations_fixed: usize,
    pub suppressed_violations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedFix {
    pub rule_id: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileFix {
    pub file: String,
    pub fixed: bool,
    pub applied_fixes: Vec<AppliedFix>,
    pub remaining_violations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassedCheck {
    pub rule_id: String,
    pub file: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub protocol: String,
    pub tool: ToolInfo,
    pub style: StyleInfo,
    pub status: String,
    pub coverage: Coverage,
    pub results: Vec<Violation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub passed: Vec<PassedCheck>,
    pub scan_errors: Vec<ScanError>,
    pub summary: Summary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<FileFix>,
}

impl Default for Report {
    fn default() -> Self {
        Self::new()
    }
}

impl Report {
    pub fn new() -> Self {
        Self {
            protocol: PROTOCOL_VERSION.to_owned(),
            tool: ToolInfo {
                name: "bits".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            style: StyleInfo {
                name: "Bits’s Coding Style".to_owned(),
                version: STYLE_VERSION.to_owned(),
            },
            status: "passed".to_owned(),
            coverage: Coverage::default(),
            results: Vec::new(),
            passed: Vec::new(),
            scan_errors: Vec::new(),
            summary: Summary::default(),
            fixes: Vec::new(),
        }
    }

    pub fn finish(&mut self) {
        self.results.sort_by(|a, b| {
            (
                &a.file,
                a.range.start.line,
                a.range.start.column,
                &a.rule_id,
            )
                .cmp(&(
                    &b.file,
                    b.range.start.line,
                    b.range.start.column,
                    &b.rule_id,
                ))
        });
        self.scan_errors
            .sort_by(|a, b| (&a.file, &a.error).cmp(&(&b.file, &b.error)));
        self.summary.violations = self.results.len();
        self.summary.fixable_violations = self.results.iter().filter(|v| v.fixable).count();
        self.summary.scan_errors = self.scan_errors.len();
        self.status = if !self.scan_errors.is_empty() {
            "error"
        } else if self.results.is_empty() {
            "passed"
        } else {
            "violation"
        }
        .to_owned();
        self.coverage.partial = self
            .coverage
            .partial
            .intersection(&self.coverage.executed)
            .cloned()
            .collect();
        self.coverage.not_applicable = self
            .coverage
            .not_applicable
            .difference(&self.coverage.executed)
            .cloned()
            .collect();
        self.coverage.not_implemented = RULE_IDS
            .iter()
            .filter(|id| !self.coverage.executed.contains(**id))
            .filter(|id| !self.coverage.not_applicable.contains(**id))
            .map(|id| (*id).to_owned())
            .collect();
    }
}
