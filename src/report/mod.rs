mod markdown;
mod structured;

use std::collections::BTreeSet;

use clap::ValueEnum;
use serde_json::Value;

use crate::model::{Report, Violation};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Compact,
    Json,
    Jsonl,
    Sarif,
    Markdown,
}

#[derive(Clone, Copy)]
pub struct RenderOptions {
    pub include_passed: bool,
    pub violations_only: bool,
}

pub fn render(
    report: &Report,
    format: OutputFormat,
    include_passed: bool,
    violations_only: bool,
) -> Result<String, String> {
    let opts = RenderOptions {
        include_passed,
        violations_only,
    };
    match format {
        OutputFormat::Text => Ok(text(report, opts)),
        OutputFormat::Compact => Ok(compact(report, opts)),
        OutputFormat::Json => structured::json_output(report, opts),
        OutputFormat::Jsonl => structured::jsonl(report, opts),
        OutputFormat::Sarif => structured::sarif(report, opts),
        OutputFormat::Markdown => Ok(markdown::render(report, opts)),
    }
}

pub(crate) fn filtered_results(
    report: &Report,
    opts: RenderOptions,
) -> Vec<&Violation> {
    report
        .results
        .iter()
        .filter(|v| !opts.violations_only || v.severity == "violation")
        .collect()
}

pub(crate) fn status_label(status: &str) -> &'static str {
    match status {
        "passed" => "PASS",
        "violation" => "FAIL",
        "warning" => "WARNING",
        "unsupported" => "UNSUPPORTED",
        _ => "ERROR",
    }
}

pub(crate) fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn split_by_severity<'a>(
    results: &[&'a Violation],
) -> (Vec<&'a Violation>, Vec<&'a Violation>) {
    let warnings = results
        .iter()
        .copied()
        .filter(|v| v.severity != "violation")
        .collect();
    let violations = results
        .iter()
        .copied()
        .filter(|v| v.severity == "violation")
        .collect();
    (warnings, violations)
}

fn needs_prove_hint(violation: &Violation) -> bool {
    !violation.improving
        && violation.explanation.is_none()
        && violation.severity == "warning"
        && (violation.rule_id == "BCS003"
            || violation.obligation.as_deref() == Some("prove_minimalism"))
}

fn render_text_violation(output: &mut String, violation: &Violation) {
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
    if !violation.hotspots.is_empty() {
        output.push_str(&format!(
            "  hotspots: {}\n",
            violation.hotspots.join(", ")
        ));
    }
    if let Some(prescription) = &violation.prescription {
        output.push_str(&format!("  prescription: {prescription}\n"));
    }
    if violation.improving {
        output.push_str(
            "  ratchet: improving (complexity reduced vs base; keep going)\n",
        );
    }
    if let Some(explanation) = &violation.explanation {
        output.push_str(&format!("  explanation: {explanation}\n"));
    }
}

fn text(report: &Report, opts: RenderOptions) -> String {
    let mut output = String::new();
    let results = filtered_results(report, opts);
    let (warnings, violations) = split_by_severity(&results);
    let hint_rules = render_text_warnings(&mut output, &warnings);
    if !violations.is_empty() && !warnings.is_empty() {
        output.push_str("\nViolations (blocking):\n");
    }
    for violation in &violations {
        render_text_violation(&mut output, violation);
    }
    for rule in hint_rules {
        output.push_str(&format!(
            "hint: prefer refactoring; if unavoidable, register bits:prove({rule}[, test=name]): <minimal reason>; <concrete risk> (budget: 1 per change)\n"
        ));
    }
    append_text_extras(&mut output, report, opts.include_passed);
    append_text_summary(&mut output, report);
    output
}

fn render_text_warnings<'a>(
    output: &mut String,
    warnings: &[&'a Violation],
) -> BTreeSet<&'a str> {
    let mut hint_rules = BTreeSet::new();
    for warning in warnings {
        render_text_violation(output, warning);
        if needs_prove_hint(warning) {
            hint_rules.insert(warning.rule_id.as_str());
        }
    }
    hint_rules
}

fn append_text_extras(
    output: &mut String,
    report: &Report,
    include_passed: bool,
) {
    for error in &report.scan_errors {
        output.push_str(&format!(
            "{}: {}: {}\n",
            error.file, error.error, error.message
        ));
    }
    for fix in report.fixes.iter().filter(|f| f.fixed) {
        let count: usize = fix.applied_fixes.iter().map(|a| a.count).sum();
        output
            .push_str(&format!("{}: fixed {} violation(s)\n", fix.file, count));
    }
    if include_passed {
        for passed in &report.passed {
            output.push_str(&format!(
                "{}: {}: passed\n",
                passed.file, passed.rule_id
            ));
        }
    }
}

fn append_text_summary(output: &mut String, report: &Report) {
    let s = &report.summary;
    let unsupported = if report.fail_on_unsupported {
        format!("Unsupported: {}\n", s.unsupported_files)
    } else {
        format!("Skipped (unsupported): {}\n", s.unsupported_files)
    };
    let stale = if s.stale_baseline_entries > 0 {
        format!(
            "Stale baseline entries: {} (run `bits style baseline --prune <file>`)\n",
            s.stale_baseline_entries
        )
    } else {
        String::new()
    };
    output.push_str(&format!(
        "Scanned: {} files\nViolations: {}\nWarnings: {}\nImproving: {}\nFixable: {}\nSuppressed: {}\n{}{}Scan errors: {}\nResult: {}\n",
        s.files_scanned,
        s.violations,
        s.warnings,
        s.improving,
        s.fixable_violations,
        s.suppressed_violations,
        unsupported,
        stale,
        s.scan_errors,
        status_label(&report.status)
    ));
}

fn compact(report: &Report, opts: RenderOptions) -> String {
    let mut output = String::new();
    let results = filtered_results(report, opts);
    let (warnings, violations) = split_by_severity(&results);
    for warning in &warnings {
        output.push_str(&compact_line('W', warning));
    }
    if !violations.is_empty() && !warnings.is_empty() {
        output.push_str("--- violations ---\n");
    }
    for violation in &violations {
        output.push_str(&compact_line('V', violation));
    }
    for error in &report.scan_errors {
        output.push_str(&format!(
            "[E] {}: {}: {}\n",
            error.file, error.error, error.message
        ));
    }
    append_compact_tail(&mut output, report);
    output
}

fn append_compact_tail(output: &mut String, report: &Report) {
    let s = &report.summary;
    output.push_str(&format!(
        "Result: {} (files={} violations={} warnings={} suppressed={} skipped_unsupported={} errors={}",
        status_label(&report.status),
        s.files_scanned,
        s.violations,
        s.warnings,
        s.suppressed_violations,
        s.unsupported_files,
        s.scan_errors
    ));
    if s.stale_baseline_entries > 0 {
        output
            .push_str(&format!(" stale_baseline={}", s.stale_baseline_entries));
    }
    output.push_str(")\n");
}

fn compact_line(tag: char, violation: &Violation) -> String {
    let mut line = format!(
        "[{tag}] {}:{}:{} {} {} (actual={} expected={})",
        violation.file,
        violation.range.start.line,
        violation.range.start.column,
        violation.rule_id,
        violation.message,
        display_value(&violation.actual),
        display_value(&violation.expected)
    );
    if !violation.hotspots.is_empty() {
        line.push_str(&format!(" [{}]", violation.hotspots.join(", ")));
    }
    if let Some(prescription) = &violation.prescription {
        line.push_str(&format!(" -> {prescription}"));
    }
    if violation.improving {
        line.push_str(" [improving]");
    }
    line.push('\n');
    line
}
