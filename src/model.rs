use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: &str = "bits.style.scan/v2";
pub const STYLE_VERSION: &str = "0.4";
pub const RULE_IDS: [&str; 17] = [
    "BCS001", "BCS002", "BCS003", "BCS004", "BCS005", "BCS006", "BCS007",
    "BCS008", "BCS009", "BCS010", "BCS011", "BCS012", "BCS013", "BCS014",
    "BCS015", "BCS016", "BCS017",
];

pub fn is_format_rule(rule: &str) -> bool {
    matches!(rule, "BCS001" | "BCS002" | "BCS007" | "BCS008" | "BCS009")
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AllowAnnotation {
    pub rule_id: String,
    pub file: String,
    pub line: usize,
    pub reason: String,
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
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default = "default_confidence")]
    pub confidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obligation: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub improving: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotspots: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prescription: Option<String>,
}

fn default_severity() -> String {
    "violation".to_owned()
}

fn default_confidence() -> String {
    "high".to_owned()
}

impl Violation {
    #[allow(clippy::too_many_arguments)]
    // bits:allow(BCS004): core diagnostic builder with fixed public signature
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
            severity: "violation".to_owned(),
            confidence: "high".to_owned(),
            explanation: None,
            category: None,
            obligation: None,
            improving: false,
            hotspots: Vec::new(),
            prescription: None,
        }
    }

    pub fn mark_high_confidence_violation(&mut self) {
        self.severity = "violation".to_owned();
        self.confidence = "high".to_owned();
        self.obligation = Some("refactor".to_owned());
    }

    pub fn mark_taste_warning(&mut self, category: &str) {
        self.severity = "warning".to_owned();
        self.confidence = "high".to_owned();
        self.category = Some(category.to_owned());
        self.obligation = Some("prove_minimalism".to_owned());
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
    pub warnings: usize,
    pub unsupported_files: usize,
    #[serde(default)]
    pub proves: usize,
    #[serde(default)]
    pub improving: usize,
    #[serde(default)]
    pub functions_checked: usize,
    #[serde(default)]
    pub functions_with_warnings: usize,
    #[serde(default)]
    pub stale_baseline_entries: usize,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passed: Vec<PassedCheck>,
    pub scan_errors: Vec<ScanError>,
    pub summary: Summary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<FileFix>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allows: Vec<AllowAnnotation>,
    #[serde(skip)]
    pub fail_on_unsupported: bool,
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
            unsupported: Vec::new(),
            allows: Vec::new(),
            fail_on_unsupported: false,
        }
    }

    pub fn finish(&mut self) {
        self.sort_records();
        self.recompute_summary();
        self.recompute_status();
        self.recompute_coverage();
    }

    fn sort_records(&mut self) {
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
        self.allows.sort_by(|a, b| {
            (&a.file, a.line, &a.rule_id).cmp(&(&b.file, b.line, &b.rule_id))
        });
    }

    fn recompute_summary(&mut self) {
        let count = |pred: fn(&Violation) -> bool| {
            self.results.iter().filter(|v| pred(v)).count()
        };
        self.summary.violations = count(|v| v.severity == "violation");
        self.summary.warnings = count(|v| v.severity == "warning");
        self.summary.proves =
            count(|v| v.obligation.as_deref() == Some("critic_review"));
        self.summary.improving = count(|v| v.improving);
        self.summary.fixable_violations = count(|v| v.fixable);
        self.summary.scan_errors = self.scan_errors.len();
    }

    fn recompute_status(&mut self) {
        self.status = if !self.scan_errors.is_empty() {
            "error"
        } else if self.summary.violations > 0 {
            "violation"
        } else if self.fail_on_unsupported && !self.unsupported.is_empty() {
            "unsupported"
        } else if self.summary.warnings > 0 {
            "warning"
        } else {
            "passed"
        }
        .to_owned();
    }

    fn recompute_coverage(&mut self) {
        let exec = &self.coverage.executed;
        self.coverage.partial =
            self.coverage.partial.intersection(exec).cloned().collect();
        self.coverage.not_applicable = self
            .coverage
            .not_applicable
            .difference(exec)
            .cloned()
            .collect();
        self.coverage.not_implemented = RULE_IDS
            .iter()
            .filter(|id| !exec.contains(**id))
            .filter(|id| !self.coverage.not_applicable.contains(**id))
            .map(|id| (*id).to_owned())
            .collect();
    }
}
