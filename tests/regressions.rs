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
    let results = scan("py", source);
    assert!(
        results
            .iter()
            .any(|result| { result["rule_id"] == "BCS013" && result["severity"] == "warning" })
    );
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

#[test]
fn diff_patch_content_cannot_impersonate_file_headers() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.py");
    fs::write(
        &file,
        "value = 0\n# keep 1\n# keep 2\n# keep 3\n# keep 4\n# keep 5\n# keep 6\n# keep 7\n# keep 8\n",
    )
    .unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["add", "sample.py"]);
    run_git(root, &["commit", "-qm", "initial"]);
    fs::write(
        &file,
        "value = 0\n++ value\n# keep 1\n# keep 2\n# keep 3\n# keep 4\n# keep 5\n# keep 6\n# keep 7\n# keep 8\nglobal_counter = 0\n",
    )
    .unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(report["summary"]["files_scanned"], 1, "{report:#}");
    assert!(report["results"].as_array().unwrap().iter().any(|result| {
        result["file"] == "sample.py"
            && result["actual"] == "global_counter = 0"
            && result["range"]["start"]["line"] == 11
    }));
}

#[test]
fn diff_ignores_configured_mnemonic_prefixes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.py");
    fs::write(&file, "value = 0\n").unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["config", "diff.mnemonicPrefix", "true"]);
    run_git(root, &["add", "sample.py"]);
    run_git(root, &["commit", "-qm", "initial"]);
    fs::write(&file, "value = 0\nglobal_counter = 0\n").unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(report["summary"]["files_scanned"], 1, "{report:#}");
    assert!(report["results"].as_array().unwrap().iter().any(|result| {
        result["file"] == "sample.py" && result["actual"] == "global_counter = 0"
    }));
}

#[test]
fn diff_combines_staged_and_unstaged_hunks_across_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let first = root.join("first.py");
    let second = root.join("second.py");
    fs::write(
        &first,
        "value = 0\n# keep 1\n# keep 2\n# keep 3\n# keep 4\n# keep 5\n# keep 6\n# keep 7\n# keep 8\n",
    )
    .unwrap();
    fs::write(&second, "value = 0\n").unwrap();
    init_diff_repo(root);

    fs::write(
        &first,
        "value = 0\nglobal_counter = 0\n# keep 1\n# keep 2\n# keep 3\n# keep 4\n# keep 5\n# keep 6\n# keep 7\n# keep 8\n",
    )
    .unwrap();
    run_git(root, &["add", "first.py"]);
    fs::write(
        &first,
        "value = 0\nglobal_counter = 0\n# keep 1\n# keep 2\n# keep 3\n# keep 4\n# keep 5\n# keep 6\n# keep 7\n# keep 8\nthird_counter = 2\n",
    )
    .unwrap();
    fs::write(&second, "value = 0\nanother_counter = 1\n").unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(1), "{report:#}");
    assert_eq!(report["summary"]["files_scanned"], 2, "{report:#}");
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 3, "{report:#}");
    for (file, actual, line) in [
        ("first.py", "global_counter = 0", 2),
        ("first.py", "third_counter = 2", 11),
        ("second.py", "another_counter = 1", 2),
    ] {
        assert!(
            results.iter().any(|result| {
                result["file"] == file
                    && result["actual"] == actual
                    && result["range"]["start"]["line"] == line
            }),
            "missing {file}:{line} {actual}: {report:#}"
        );
    }
}

#[test]
fn diff_skips_pure_deletions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("sample.py"), "global_counter = 0\nvalue = 0\n").unwrap();
    init_diff_repo(root);
    fs::write(root.join("sample.py"), "value = 0\n").unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(0), "{report:#}");
    assert_eq!(report["summary"]["files_scanned"], 0);
    assert!(report["results"].as_array().unwrap().is_empty());
}

#[test]
fn diff_scans_non_ascii_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("模块.py"), "value = 0\n").unwrap();
    init_diff_repo(root);
    fs::write(root.join("模块.py"), "value = 0\nglobal_counter = 0\n").unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(1), "{report:#}");
    assert_eq!(report["results"][0]["file"], "模块.py");
    assert_eq!(report["results"][0]["actual"], "global_counter = 0");
}

#[test]
fn diff_reports_added_lines_in_configured_renames() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(
        root.join("old.py"),
        "value = 0\n# keep 1\n# keep 2\n# keep 3\n# keep 4\n",
    )
    .unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["config", "diff.renames", "true"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "initial"]);
    run_git(root, &["mv", "old.py", "renamed.py"]);
    fs::write(
        root.join("renamed.py"),
        "value = 0\n# keep 1\n# keep 2\n# keep 3\n# keep 4\nglobal_counter = 0\n",
    )
    .unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(1), "{report:#}");
    assert_eq!(report["summary"]["files_scanned"], 1);
    assert_eq!(report["results"].as_array().unwrap().len(), 1, "{report:#}");
    assert_eq!(report["results"][0]["file"], "renamed.py");
    assert_eq!(
        report["results"][0]["actual"], "global_counter = 0",
        "{report:#}"
    );
}

#[test]
fn diff_bypasses_configured_textconv() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("sample.py"), "value = 0\n").unwrap();
    fs::write(root.join(".gitattributes"), "*.py diff=broken\n").unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["config", "diff.broken.textconv", "false"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "initial"]);
    fs::write(root.join("sample.py"), "value = 0\nglobal_counter = 0\n").unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    let report = json(&output);
    assert_eq!(output.status.code(), Some(1), "{report:#}");
    assert_eq!(report["results"][0]["actual"], "global_counter = 0");
}

#[cfg(unix)]
#[test]
fn diff_does_not_walk_unrelated_directories_for_dependency_context() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("sample.ts");
    fs::write(&file, "export function value() {\n  return 1;\n}\n").unwrap();
    fs::create_dir(root.join("blocked")).unwrap();
    fs::write(root.join("blocked/context.ts"), "export const value = 1;\n").unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "initial"]);
    fs::write(
        &file,
        "export function value() {\n  return 1;\n}\n\nexport const extraValue = 1;\n",
    )
    .unwrap();
    let blocked = root.join("blocked");
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();
    let report = json(&output);
    assert_eq!(output.status.code(), Some(0), "{report:#}");
    assert_eq!(report["summary"]["files_scanned"], 1);
}

fn init_diff_repo(root: &Path) {
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "initial"]);
}

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}
