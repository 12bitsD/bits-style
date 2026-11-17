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

fn scan(extension: &str, source: &str) -> Vec<Value> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(format!("sample.{extension}"));
    fs::write(&path, source).unwrap();
    let output = bits(&[
        "style",
        "report",
        path.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)["results"].as_array().unwrap().clone()
}

fn assert_no_rule(results: &[Value], rule: &str) {
    assert!(
        !results.iter().any(|result| result["rule_id"] == rule),
        "unexpected {rule}: {results:#?}"
    );
}

#[test]
fn fix_preserves_space_at_multiline_string_opening_line() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sample.py");
    fs::write(
        &path,
        "def message():\n\treturn \"\"\"first   \n    keep  \nlast\"\"\"",
    )
    .unwrap();
    let output = bits(&["style", "fix", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "def message():\n    return \"\"\"first   \n    keep  \nlast\"\"\"\n"
    );
}

#[test]
fn baseline_consumes_duplicate_fingerprints_one_by_one() {
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

    fs::write(&source, "global_counter = 0\nglobal_counter = 0\n").unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(report["summary"]["suppressed_violations"], 1);
    assert_eq!(report["results"].as_array().unwrap().len(), 1);
}

#[test]
fn diff_no_ignore_still_excludes_dependency_and_build_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("sample.py"), "global_counter = 0\n").unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["add", "sample.py"]);
    run_git(root, &["commit", "-qm", "initial"]);

    fs::create_dir(root.join("target")).unwrap();
    fs::create_dir(root.join("node_modules")).unwrap();
    fs::write(root.join("target/generated.py"), "target_counter = 0\n").unwrap();
    fs::write(
        root.join("node_modules/dependency.py"),
        "dependency_counter = 0\n",
    )
    .unwrap();
    fs::write(root.join("new.py"), "new_counter = 0\n").unwrap();
    let output = bits(&[
        "style",
        "report",
        root.to_str().unwrap(),
        "--diff",
        "--no-ignore",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(report["summary"]["files_scanned"], 1);
    assert_eq!(report["results"][0]["file"], "new.py");
}

#[test]
fn unused_import_ignores_comments_and_strings() {
    let results = scan("py", "import os\n# os\nVALUE = \"os\"\n");
    assert!(results.iter().any(|result| result["rule_id"] == "BCS014"));
}

#[test]
fn catch_all_translation_is_allowed() {
    let source = "def parse(value):\n    try:\n        return int(value)\n    except Exception:\n        raise ValueError(\"invalid\")\n";
    assert_no_rule(&scan("py", source), "BCS013");
}

#[test]
fn element_assignment_is_not_parameter_reassignment() {
    let source = "def update(values):\n    values[0] = 1\n    return values\n";
    assert_no_rule(&scan("py", source), "BCS011");
}

#[test]
fn uppercase_mutable_globals_are_rejected() {
    let results = scan("py", "CACHE = []\n");
    assert!(results.iter().any(|result| result["rule_id"] == "BCS011"));
}

#[test]
fn bare_return_in_catch_is_not_a_forged_value() {
    let source = "function parse(value) {\n  try {\n    return Number(value);\n  } catch (error) {\n    return;\n  }\n}\n";
    assert_no_rule(&scan("js", source), "BCS013");
}

#[test]
fn switch_opening_brace_is_checked() {
    let source = "int classify(int value)\n{\n\tswitch (value)\n\t{\n\tcase 1:\n\t\treturn 1;\n\tdefault:\n\t\treturn 0;\n\t}\n}\n";
    let results = scan("c", source);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS007"));
}

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}
