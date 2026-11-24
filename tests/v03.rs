use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn bits(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bits"))
        .args(args)
        .output()
        .unwrap()
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn init_repo(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "bits@example.invalid"]);
    git(root, &["config", "user.name", "Bits Tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "initial"]);
}

#[test]
fn bcs003_is_a_warning_and_carries_explanation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.js");
    let source = format!(
        "// bits:explain(BCS003): cohesive transition; splitting would hide invariants\nfunction saveProfile() {{\n{}  return true;\n}}\n",
        "  validate();\n".repeat(48)
    );
    fs::write(&path, source).unwrap();
    let output = bits(&["style", "check", path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = json(&output);
    assert_eq!(report["status"], "warning");
    assert_eq!(report["summary"]["violations"], 0);
    assert_eq!(report["summary"]["warnings"], 1);
    assert_eq!(
        report["results"][0]["explanation"],
        "cohesive transition; splitting would hide invariants"
    );
}

#[test]
fn unsupported_languages_are_explicit_and_do_not_pass() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("lib.rs"), "fn main() {}\n").unwrap();
    fs::write(directory.path().join("main.go"), "package main\n").unwrap();
    let report = bits(&[
        "style",
        "report",
        directory.path().to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(report.status.code(), Some(0));
    let report = json(&report);
    assert_eq!(report["status"], "unsupported");
    assert_eq!(report["summary"]["unsupported_files"], 1);
    assert_eq!(report["unsupported"], serde_json::json!(["main.go"]));

    let check = bits(&[
        "style",
        "check",
        directory.path().to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(check.status.code(), Some(2));
}

#[test]
fn tsx_react_and_type_imports_have_no_false_positives() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("App.tsx");
    let source = r#"import React, { useState } from "react";
import type { Tab } from "./types";

export function App({ enabled }: {
  enabled: boolean;
}) {
  const [tab] = useState<Tab>("home");
  void React;
  return <button disabled={!enabled}>{tab}</button>;
}
"#;
    fs::write(&path, source).unwrap();
    let output = bits(&[
        "style",
        "report",
        path.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert!(
        report["results"].as_array().unwrap().is_empty(),
        "{report:#}"
    );
}

#[test]
fn fix_diff_only_changes_added_lines() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.py");
    fs::write(
        &file,
        "def outer(flag):\n\tif flag:\n        return 1\n    return 0\n",
    )
    .unwrap();
    init_repo(root);
    fs::write(
        &file,
        "def outer(flag):\n\tif flag:\n        return 2\n    return 0\n",
    )
    .unwrap();

    let output = bits(&[
        "style",
        "fix",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let content = fs::read_to_string(&file).unwrap();
    assert!(content.contains("\tif flag:"), "{content}");
    assert!(content.contains("return 2"), "{content}");
}

#[test]
fn fix_diff_preserves_an_unchanged_missing_eof_newline() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.py");
    fs::write(&file, "def outer():\n    return 0").unwrap();
    init_repo(root);
    let changed = "def outer():\n    value = 1\n    return 0";
    fs::write(&file, changed).unwrap();

    let output = bits(&[
        "style",
        "fix",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&file).unwrap(), changed);
    let report = json(&output);
    assert_eq!(report["summary"]["violations_fixed"], 0);
    assert!(!report["fixes"][0]["fixed"].as_bool().unwrap());
    assert!(
        report["fixes"][0]["applied_fixes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn fix_diff_repairs_an_added_eof_line_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.py");
    fs::write(&file, "def outer():\n    return 0\n").unwrap();
    init_repo(root);
    fs::write(&file, "def outer():\n    value = 1\n    return value").unwrap();

    let first = bits(&[
        "style",
        "fix",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(first.status.code(), Some(0));
    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "def outer():\n    value = 1\n    return value\n");
    let first_report = json(&first);
    assert_eq!(first_report["summary"]["violations_fixed"], 1);
    assert!(first_report["fixes"][0]["fixed"].as_bool().unwrap());
    assert_eq!(first_report["fixes"][0]["applied_fixes"][0]["count"], 1);

    let second = bits(&[
        "style",
        "fix",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&file).unwrap(), fixed);
    let second_report = json(&second);
    assert!(!second_report["fixes"][0]["fixed"].as_bool().unwrap());
    assert_eq!(second_report["summary"]["violations_fixed"], 0);
    assert!(
        second_report["fixes"][0]["applied_fixes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn commit_options_are_rejected_instead_of_silently_passing() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("sample.py"), "global_counter = 0\n").unwrap();
    init_repo(root);
    fs::write(
        root.join("sample.py"),
        "global_counter = 0\nanother_counter = 1\n",
    )
    .unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--commit=--name-only",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid diff commit"));
}

#[test]
fn quoted_filenames_are_scanned_in_diff_mode() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("a b.py");
    fs::write(&file, "global_counter = 0\n").unwrap();
    init_repo(root);
    fs::write(&file, "global_counter = 0\nanother_counter = 1\n").unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(report["summary"]["files_scanned"], 1);
    assert_eq!(report["results"][0]["actual"], "another_counter = 1");
}

#[test]
fn baseline_remaining_count_uses_final_filtered_results() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let baseline = root.join("baseline.json");
    fs::write(root.join("sample.py"), "global_counter = 0\n").unwrap();
    let generated = bits(&[
        "style",
        "report",
        root.to_str().unwrap(),
        "--output",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(generated.status.code(), Some(0));
    let output = bits(&[
        "style",
        "fix",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert!(report["results"].as_array().unwrap().is_empty());
    assert_eq!(report["fixes"][0]["remaining_violations"], 0);
}

#[test]
fn c_return_value_rule_is_not_implemented_not_not_applicable() {
    let output = bits(&[
        "style",
        "report",
        "tests/fixtures/valid/sample.c",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert!(
        report["coverage"]["not_implemented"]
            .as_array()
            .unwrap()
            .contains(&Value::String("BCS013".to_owned()))
    );
}

#[test]
fn python_variables_follow_snake_case() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.py");
    fs::write(
        &path,
        "def update():\n    badName = 1\n    return badName\n",
    )
    .unwrap();
    let output = bits(&["style", "check", path.to_str().unwrap(), "--format", "json"]);
    assert!(
        json(&output)["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| { result["rule_id"] == "BCS010" && result["actual"] == "badName" })
    );
}
