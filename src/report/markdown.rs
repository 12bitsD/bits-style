use crate::model::{Report, Violation};
use crate::report::{RenderOptions, display_value, status_label};

pub(crate) fn render(report: &Report, opts: RenderOptions) -> String {
    let mut output = String::new();
    let s = &report.summary;
    output.push_str(&format!(
        "## Bits Style Summary: {}\n\n- Scanned: `{}` files | Violations: `{}` | Warnings: `{}` | Proves: `{}` | Allows: `{}` | Skipped (unsupported): `{}`\n",
        status_label(&report.status),
        s.files_scanned,
        s.violations,
        s.warnings,
        s.proves,
        report.allows.len(),
        s.unsupported_files
    ));
    if s.stale_baseline_entries > 0 {
        output.push_str(&format!(
            "- Stale baseline entries: `{}` (run `bits style baseline --prune <file>`)\n",
            s.stale_baseline_entries
        ));
    }
    append_violations(&mut output, report);
    append_annotations(&mut output, report);
    if !opts.violations_only {
        append_warnings(&mut output, report);
    }
    output
}

fn append_violations(output: &mut String, report: &Report) {
    let violations: Vec<&Violation> = report
        .results
        .iter()
        .filter(|v| v.severity == "violation")
        .collect();
    if violations.is_empty() {
        return;
    }
    output.push_str("\n### Violations\n\n");
    for v in violations {
        let mut item = format!(
            "- `{}:{}` `{}`: {} (`actual={}`, `expected={}`)",
            v.file,
            v.range.start.line,
            v.rule_id,
            v.message,
            display_value(&v.actual),
            display_value(&v.expected)
        );
        if !v.hotspots.is_empty() {
            item.push_str(&format!(" — hotspots: {}", v.hotspots.join(", ")));
        }
        if let Some(prescription) = &v.prescription {
            item.push_str(&format!(" — fix: {prescription}"));
        }
        item.push('\n');
        output.push_str(&item);
    }
}

fn append_annotations(output: &mut String, report: &Report) {
    let proves: Vec<&Violation> = report
        .results
        .iter()
        .filter(|v| v.obligation.as_deref() == Some("critic_review"))
        .collect();
    if !proves.is_empty() {
        output.push_str("\n### `bits:prove` Review Checklist\n\n");
        for v in proves {
            output.push_str(&format!(
                "- `{}:{}` `{}`: {}\n",
                v.file,
                v.range.start.line,
                v.rule_id,
                v.explanation.as_deref().unwrap_or("")
            ));
        }
    }
    if !report.allows.is_empty() {
        output.push_str("\n### `bits:allow` Exemptions\n\n");
        for allow in &report.allows {
            output.push_str(&format!(
                "- `{}:{}` `{}`: {}\n",
                allow.file, allow.line, allow.rule_id, allow.reason
            ));
        }
    }
}

fn append_warnings(output: &mut String, report: &Report) {
    let other: Vec<&Violation> = report
        .results
        .iter()
        .filter(|v| {
            v.severity == "warning"
                && v.obligation.as_deref() != Some("critic_review")
        })
        .collect();
    if other.is_empty() {
        return;
    }
    output.push_str(&format!("\n### Warnings ({})\n\n", other.len()));
    for v in other.iter().take(20) {
        output.push_str(&format!(
            "- `{}:{}` `{}`: {} (`actual={}`)\n",
            v.file,
            v.range.start.line,
            v.rule_id,
            v.message,
            display_value(&v.actual)
        ));
    }
    if other.len() > 20 {
        output.push_str(&format!(
            "- ... and {} more warning(s)\n",
            other.len() - 20
        ));
    }
}
