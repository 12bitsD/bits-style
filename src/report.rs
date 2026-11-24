use serde_json::{Value, json};

use crate::model::{RULE_IDS, Report, Violation};
use crate::scanner::OutputFormat;

pub fn render(
    report: &Report,
    format: OutputFormat,
    include_passed: bool,
) -> Result<String, String> {
    match format {
        OutputFormat::Text => Ok(text(report, include_passed)),
        OutputFormat::Json => serde_json::to_string_pretty(report)
            .map(|output| output + "\n")
            .map_err(|error| error.to_string()),
        OutputFormat::Jsonl => jsonl(report, include_passed),
        OutputFormat::Sarif => serde_json::to_string_pretty(&sarif(report))
            .map(|output| output + "\n")
            .map_err(|error| error.to_string()),
    }
}

fn text(report: &Report, include_passed: bool) -> String {
    let mut output = String::new();
    for violation in &report.results {
        output.push_str(&format!(
            "{}:{}:{}: {}: {} actual={} expected={} fixable={} severity={} confidence={}\n",
            violation.file,
            violation.range.start.line,
            violation.range.start.column,
            violation.rule_id,
            violation.message,
            display_value(&violation.actual),
            display_value(&violation.expected),
            if violation.fixable { "yes" } else { "no" },
            violation.severity,
            violation.confidence
        ));
        if let Some(explanation) = &violation.explanation {
            output.push_str(&format!("  explanation: {explanation}\n"));
        } else if violation.severity == "warning"
            && (violation.rule_id == "BCS003"
                || violation.obligation.as_deref() == Some("prove_minimalism"))
        {
            output.push_str(&format!(
                "  hint: prefer refactoring; if unavoidable, register bits:prove({}[, test=name]): <minimal reason>; <concrete risk> (budget: 1 per change)\n",
                violation.rule_id
            ));
        }
    }
    for error in &report.scan_errors {
        output.push_str(&format!(
            "{}: {}: {}\n",
            error.file, error.error, error.message
        ));
    }
    for fix in &report.fixes {
        if fix.fixed {
            let count: usize = fix.applied_fixes.iter().map(|applied| applied.count).sum();
            output.push_str(&format!("{}: fixed {} violation(s)\n", fix.file, count));
        }
    }
    if include_passed {
        for passed in &report.passed {
            output.push_str(&format!("{}: {}: passed\n", passed.file, passed.rule_id));
        }
    }
    output.push_str(&format!(
        "Scanned: {} files\nViolations: {}\nWarnings: {}\nFixable: {}\nSuppressed: {}\nUnsupported: {}\nScan errors: {}\nResult: {}\n",
        report.summary.files_scanned,
        report.summary.violations,
        report.summary.warnings,
        report.summary.fixable_violations,
        report.summary.suppressed_violations,
        report.summary.unsupported_files,
        report.summary.scan_errors,
        match report.status.as_str() {
            "passed" => "PASS",
            "violation" => "FAIL",
            "warning" => "WARNING",
            "unsupported" => "UNSUPPORTED",
            _ => "ERROR",
        }
    ));
    output
}

fn jsonl(report: &Report, include_passed: bool) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push(
        serde_json::to_string(&json!({
            "type": "run",
            "protocol": report.protocol,
            "style_version": report.style.version,
            "coverage": report.coverage,
        }))
        .map_err(|error| error.to_string())?,
    );
    for violation in &report.results {
        lines.push(
            serde_json::to_string(&json!({
                "type": violation.severity,
                "rule_id": violation.rule_id,
                "file": violation.file,
                "line": violation.range.start.line,
                "column": violation.range.start.column,
                "message": violation.message,
                "actual": violation.actual,
                "expected": violation.expected,
                "fixable": violation.fixable,
                "severity": violation.severity,
                "confidence": violation.confidence,
                "explanation": violation.explanation,
                "category": violation.category,
                "obligation": violation.obligation,
            }))
            .map_err(|error| error.to_string())?,
        );
    }
    for error in &report.scan_errors {
        lines.push(
            serde_json::to_string(&json!({
                "type": "scan_error",
                "file": error.file,
                "error": error.error,
                "message": error.message,
            }))
            .map_err(|error| error.to_string())?,
        );
    }
    if include_passed {
        for passed in &report.passed {
            lines.push(serde_json::to_string(passed).map_err(|error| error.to_string())?);
        }
    }
    lines.push(
        serde_json::to_string(&json!({
            "type": "summary",
            "status": report.status,
            "files_scanned": report.summary.files_scanned,
            "violations": report.summary.violations,
            "scan_errors": report.summary.scan_errors,
            "files_fixed": report.summary.files_fixed,
            "violations_fixed": report.summary.violations_fixed,
            "suppressed_violations": report.summary.suppressed_violations,
            "warnings": report.summary.warnings,
            "unsupported_files": report.summary.unsupported_files,
        }))
        .map_err(|error| error.to_string())?,
    );
    Ok(lines.join("\n") + "\n")
}

fn sarif(report: &Report) -> Value {
    let rules: Vec<Value> = RULE_IDS
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "name": id,
                "shortDescription": { "text": format!("Bits’s Coding Style {id}") }
            })
        })
        .collect();
    let results: Vec<Value> = report.results.iter().map(sarif_result).collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "bits",
                    "version": report.tool.version,
                    "informationUri": "https://github.com/12bitsD/bits-style",
                    "rules": rules,
                }
            },
            "properties": {
                "protocol": report.protocol,
                "styleVersion": report.style.version,
                "coverage": report.coverage,
                "summary": report.summary,
            },
            "results": results,
        }]
    })
}

fn sarif_result(violation: &Violation) -> Value {
    json!({
        "ruleId": violation.rule_id,
        "level": if violation.severity == "warning" { "note" } else { "error" },
        "message": { "text": violation.message },
        "properties": {
            "actual": violation.actual,
            "expected": violation.expected,
            "fixable": violation.fixable,
            "severity": violation.severity,
            "confidence": violation.confidence,
            "explanation": violation.explanation,
            "category": violation.category,
            "obligation": violation.obligation,
        },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": violation.file },
                "region": {
                    "startLine": violation.range.start.line,
                    "startColumn": violation.range.start.column,
                    "endLine": violation.range.end.line,
                    "endColumn": violation.range.end.column,
                }
            }
        }]
    })
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
