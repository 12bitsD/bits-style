use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn bits(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bits"))
        .args(args)
        .output()
        .expect("bits must run")
}

fn fixture(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn rule_ids(output: &Output) -> Vec<String> {
    json(output)["results"]
        .as_array()
        .expect("results must be an array")
        .iter()
        .map(|result| result["rule_id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn valid_files_pass_in_every_reference_language() {
    for file in [
        "valid/sample.py",
        "valid/sample.c",
        "valid/sample.cpp",
        "valid/sample.js",
        "valid/sample.ts",
        "valid/sample.tsx",
    ] {
        let output = bits(&["style", "check", &fixture(file)]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            json(&bits(&[
                "style",
                "check",
                &fixture(file),
                "--format",
                "json"
            ]))["results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn invalid_files_report_expected_rule_ids() {
    for (file, expected) in [
        (
            "invalid/sample.py",
            [
                "BCS001", "BCS004", "BCS005", "BCS010", "BCS011", "BCS014", "BCS015",
            ]
            .as_slice(),
        ),
        (
            "invalid/sample.c",
            ["BCS001", "BCS004", "BCS009", "BCS010", "BCS011", "BCS012"].as_slice(),
        ),
        (
            "invalid/sample.js",
            [
                "BCS001", "BCS004", "BCS005", "BCS009", "BCS010", "BCS011", "BCS014",
            ]
            .as_slice(),
        ),
    ] {
        let output = bits(&["style", "check", &fixture(file), "--format", "json"]);
        assert_eq!(output.status.code(), Some(1));
        let ids = rule_ids(&output);
        for expected in expected {
            assert!(
                ids.iter().any(|id| id == expected),
                "missing {expected} in {ids:?}"
            );
        }
    }
}

#[test]
fn check_and_report_share_the_same_results() {
    let check = bits(&[
        "style",
        "check",
        "tests/fixtures/invalid",
        "--format",
        "json",
    ]);
    let report = bits(&[
        "style",
        "report",
        "tests/fixtures/invalid",
        "--format",
        "json",
    ]);
    assert_eq!(check.status.code(), Some(1));
    assert_eq!(report.status.code(), Some(0));
    assert_eq!(json(&check)["results"], json(&report)["results"]);
}

#[test]
fn report_validates_against_the_public_schema() {
    let output = bits(&[
        "style",
        "report",
        "tests/fixtures/invalid",
        "--format",
        "json",
    ]);
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/report.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    validator
        .validate(&json(&output))
        .expect("report must match schema");
}

#[test]
fn dependency_cycles_return_one() {
    let output = bits(&["style", "check", "tests/fixtures/cycle", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(rule_ids(&output).contains(&"BCS014".to_owned()));
}

#[test]
fn parse_errors_return_two() {
    let output = bits(&[
        "style",
        "check",
        "tests/fixtures/parse_error",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["status"], "error");
    assert_eq!(json(&output)["summary"]["scan_errors"], 1);
}

#[test]
fn fix_is_effective_and_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.py");
    fs::write(
        &path,
        "def add_numbers(left, right):\n\treturn left + right   ",
    )
    .unwrap();

    let first = bits(&["style", "fix", path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let fixed = fs::read_to_string(&path).unwrap();
    assert_eq!(
        fixed,
        "def add_numbers(left, right):\n    return left + right\n"
    );
    assert_eq!(json(&first)["summary"]["files_fixed"], 1);
    assert_eq!(json(&first)["summary"]["violations_fixed"], 3);

    let second = bits(&["style", "fix", path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(fs::read_to_string(&path).unwrap(), fixed);
    assert_eq!(json(&second)["summary"]["files_fixed"], 0);
}

#[test]
fn fix_preserves_nested_python_indentation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.py");
    fs::write(
        &path,
        "def outer(flag):\n\tif flag:\n\t\treturn 1\n\treturn 0\n",
    )
    .unwrap();
    let output = bits(&["style", "fix", path.to_str().unwrap(), "--format", "json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "def outer(flag):\n    if flag:\n        return 1\n    return 0\n"
    );
    assert_eq!(json(&output)["summary"]["scan_errors"], 0);
}

#[test]
fn fix_preserves_multiline_string_contents() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.py");
    let source = "def message():\n    return \"\"\"first\n    keep  \nlast\"\"\"";
    fs::write(&path, source).unwrap();
    let output = bits(&["style", "fix", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "def message():\n    return \"\"\"first\n    keep  \nlast\"\"\"\n"
    );
}

#[test]
fn parallel_and_serial_results_are_identical() {
    let serial = bits(&[
        "style",
        "report",
        "tests/fixtures",
        "--jobs",
        "1",
        "--format",
        "json",
    ]);
    let parallel = bits(&[
        "style",
        "report",
        "tests/fixtures",
        "--jobs",
        "8",
        "--format",
        "json",
    ]);
    assert_eq!(json(&serial)["results"], json(&parallel)["results"]);
    assert_eq!(json(&serial)["scan_errors"], json(&parallel)["scan_errors"]);
}

#[test]
fn jsonl_and_sarif_are_machine_readable() {
    let jsonl = bits(&[
        "style",
        "report",
        "tests/fixtures/cycle",
        "--format",
        "jsonl",
    ]);
    assert_eq!(jsonl.status.code(), Some(0));
    let lines: Vec<Value> = String::from_utf8_lossy(&jsonl.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.first().unwrap()["type"], "run");
    assert_eq!(lines.last().unwrap()["type"], "summary");
    assert!(lines.iter().any(|line| line["type"] == "violation"));

    let sarif = bits(&[
        "style",
        "report",
        "tests/fixtures/cycle",
        "--format",
        "sarif",
    ]);
    assert_eq!(sarif.status.code(), Some(0));
    let sarif = json(&sarif);
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(sarif["runs"][0]["results"][0]["ruleId"], "BCS014");
}

#[test]
fn include_passed_emits_passed_rule_file_pairs() {
    let output = bits(&[
        "style",
        "report",
        &fixture("valid/sample.py"),
        "--format",
        "json",
        "--include-passed",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(json(&output)["passed"].as_array().unwrap().len() > 5);
}

#[test]
fn config_excludes_files_from_discovery() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("keep.py"), "def valid():\n\treturn 1\n").unwrap();
    fs::write(root.join("skip.py"), "def invalid():\n        return 1\n").unwrap();
    let config = root.join("bits.toml");
    fs::write(&config, "exclude = [\"skip.py\"]\n").unwrap();
    let output = bits(&[
        "style",
        "report",
        root.to_str().unwrap(),
        "--config",
        config.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["summary"]["files_scanned"], 1);
}

#[test]
fn changed_and_staged_scopes_use_git_diff() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.py");
    fs::write(&file, "def valid():\n\treturn 1\n").unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["add", "sample.py"]);
    run_git(root, &["commit", "-qm", "initial"]);

    fs::write(&file, "def valid():\n        return 1\n").unwrap();
    let changed = bits(&["style", "report", root.to_str().unwrap(), "--changed"]);
    assert_eq!(json(&changed)["summary"]["files_scanned"], 1);

    run_git(root, &["add", "sample.py"]);
    fs::write(&file, "def valid():\n    return 2\n").unwrap();
    let staged = bits(&["style", "report", root.to_str().unwrap(), "--staged"]);
    assert_eq!(json(&staged)["summary"]["files_scanned"], 1);
}

#[test]
fn output_file_receives_machine_report() {
    let directory = tempfile::tempdir().unwrap();
    let output_path: PathBuf = directory.path().join("report.json");
    let output = bits(&[
        "style",
        "report",
        &fixture("valid/sample.py"),
        "--output",
        output_path.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let report: Value = serde_json::from_str(&fs::read_to_string(output_path).unwrap()).unwrap();
    assert_eq!(report["status"], "passed");
}

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn inline_suppression_is_scoped_and_validated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.py");
    fs::write(
        &path,
        "# bits:allow(BCS011): legacy callback\nglobal_counter = 0\nother_counter = 1\n# bits:allow: missing rule\n",
    )
    .unwrap();
    let output = bits(&[
        "style",
        "report",
        path.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(report["status"], "violation");
    assert!(!report["results"].as_array().unwrap().iter().any(|result| {
        result["rule_id"] == "BCS011" && result["actual"] == "global_counter = 0"
    }));
    assert!(report["results"].as_array().unwrap().iter().any(|result| {
        result["rule_id"] == "BCS011" && result["actual"] == "other_counter = 1"
    }));
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| { result["rule_id"] == "BCS015" })
    );
}

#[test]
fn baseline_suppresses_only_exact_existing_violations() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let source = root.join("sample.py");
    let baseline = root.join("baseline.json");
    fs::write(&source, "global_counter = 0\n").unwrap();
    let generated = bits(&[
        "style",
        "report",
        root.to_str().unwrap(),
        "--output",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(generated.status.code(), Some(0));

    let clean = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(clean.status.code(), Some(0));
    assert_eq!(json(&clean)["summary"]["suppressed_violations"], 1);

    fs::write(&source, "global_counter = 0\nanother_counter = 1\n").unwrap();
    let changed = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(changed.status.code(), Some(1));
    let results = json(&changed);
    assert_eq!(results["results"].as_array().unwrap().len(), 1);
    assert_eq!(results["results"][0]["actual"], "another_counter = 1");

    fs::write(root.join("new.py"), "new_counter = 2\n").unwrap();
    let new_file = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        json(&new_file)["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| { result["file"] == "new.py" && result["rule_id"] == "BCS011" })
    );
}
