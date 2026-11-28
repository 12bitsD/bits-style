use serde_json::{Value, json};

use crate::model::{RULE_IDS, Report, Violation};
use crate::report::{RenderOptions, filtered_results};

pub(crate) fn json_output(
    report: &Report,
    opts: RenderOptions,
) -> Result<String, String> {
    if !opts.violations_only {
        return serde_json::to_string_pretty(report)
            .map(|output| output + "\n")
            .map_err(|error| error.to_string());
    }
    let mut filtered = report.clone();
    filtered.results.retain(|v| v.severity == "violation");
    serde_json::to_string_pretty(&filtered)
        .map(|output| output + "\n")
        .map_err(|error| error.to_string())
}

pub(crate) fn jsonl(
    report: &Report,
    opts: RenderOptions,
) -> Result<String, String> {
    let mut lines = Vec::new();
    lines.push(to_line(&json!({
        "type": "run",
        "protocol": report.protocol,
        "style_version": report.style.version,
        "coverage": report.coverage,
    }))?);
    for violation in filtered_results(report, opts) {
        lines.push(to_line(&jsonl_violation(violation))?);
    }
    for error in &report.scan_errors {
        lines.push(to_line(&json!({
            "type": "scan_error",
            "file": error.file,
            "error": error.error,
            "message": error.message,
        }))?);
    }
    if opts.include_passed {
        for passed in &report.passed {
            lines.push(to_line(passed)?);
        }
    }
    lines.push(to_line(&jsonl_summary(report))?);
    Ok(lines.join("\n") + "\n")
}

fn jsonl_violation(violation: &Violation) -> Value {
    json!({
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
        "improving": violation.improving,
        "hotspots": violation.hotspots,
        "prescription": violation.prescription,
    })
}

fn jsonl_summary(report: &Report) -> Value {
    let s = &report.summary;
    json!({
        "type": "summary",
        "status": report.status,
        "files_scanned": s.files_scanned,
        "violations": s.violations,
        "scan_errors": s.scan_errors,
        "files_fixed": s.files_fixed,
        "violations_fixed": s.violations_fixed,
        "suppressed_violations": s.suppressed_violations,
        "warnings": s.warnings,
        "unsupported_files": s.unsupported_files,
        "stale_baseline_entries": s.stale_baseline_entries,
    })
}

pub(crate) fn sarif(
    report: &Report,
    opts: RenderOptions,
) -> Result<String, String> {
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
    let results: Vec<Value> = filtered_results(report, opts)
        .into_iter()
        .map(sarif_result)
        .collect();
    let doc = json!({
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
    });
    serde_json::to_string_pretty(&doc)
        .map(|output| output + "\n")
        .map_err(|error| error.to_string())
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
            "improving": violation.improving,
            "hotspots": violation.hotspots,
            "prescription": violation.prescription,
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

fn to_line(value: &impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}
