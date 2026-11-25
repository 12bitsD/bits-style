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

#[test]
fn hard_function_violations_have_high_confidence() {
    let nested = "function nested(value) {\n  if (value) {\n    for (const item of value) {\n      while (item) {\n        if (item) {\n          return 1;\n        }\n      }\n    }\n  }\n  return 0;\n}\n";
    let results = scan("js", nested);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS003"
            && result["severity"] == "violation"
            && result["confidence"] == "high"
    }));

    let complex = format!(
        "function complex(value) {{\n{}  return value;\n}}\n",
        "  if (value) {\n    value += 1;\n  }\n".repeat(10)
    );
    let results = scan("js", &complex);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS003"
            && result["severity"] == "violation"
            && result["confidence"] == "high"
    }));

    let results = scan(
        "ts",
        "function run(enabled: boolean) {\n  return enabled;\n}\n",
    );
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS004"
            && result["severity"] == "violation"
            && result["confidence"] == "high"
    }));

    let results = scan(
        "ts",
        "function sum(a: number, b: number, c: number, d: number, e: number) {\n  return a + b + c + d + e;\n}\n",
    );
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS004"
            && result["severity"] == "warning"
            && result["confidence"] == "high"
    }));
}

#[test]
fn bcs003_text_report_prompts_for_concrete_explanation() {
    let directory = tempfile::tempdir().unwrap();
    let plain = directory.path().join("plain.py");
    let body = "    value = 1\n".repeat(49);
    fs::write(
        &plain,
        format!("def plain() -> int:\n{body}    return value\n"),
    )
    .unwrap();
    let output = bits(&["style", "check", plain.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains(
        "hint: prefer refactoring; if unavoidable, register bits:prove(BCS003[, test=name]): <minimal reason>; <concrete risk> (budget: 1 per change)"
    ));

    let explained = directory.path().join("explained.py");
    fs::write(
        &explained,
        format!(
            "# bits:explain(BCS003): state transition; cursor and checksum mutate together; splitting splits the lock\ndef explained() -> int:\n{body}    return value\n"
        ),
    )
    .unwrap();
    let output = bits(&["style", "check", explained.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("explanation: state transition; cursor and checksum mutate together; splitting splits the lock"));
    assert!(!text.contains("hint: prefer refactoring"));
}

#[test]
fn bcs016_detects_only_exhaustive_pure_branch_head_duplication() {
    let duplicated = "async def run(command: str, h: object) -> int:\n    if command == \"a\":\n        await h.check()\n        return 1\n    elif command == \"b\":\n        await h.check()\n        return 2\n    else:\n        await h.check()\n        return 3\n";
    let results = scan("py", duplicated);
    assert_eq!(results.len(), 1, "{results:#?}");
    assert_eq!(results[0]["rule_id"], "BCS016");
    assert_eq!(results[0]["severity"], "warning");
    assert_eq!(results[0]["category"], "symmetry");
    assert_eq!(results[0]["obligation"], "prove_minimalism");

    let js = "function run(mode) {\n  if (mode === 1) {\n    setup();\n    return 1;\n  } else {\n    setup();\n    return 2;\n  }\n}\n";
    let results = scan("js", js);
    assert!(results.iter().any(|r| r["rule_id"] == "BCS016"));

    let no_else = "def run(command: str, h: object) -> None:\n    if command == \"a\":\n        h.check()\n    elif command == \"b\":\n        h.check()\n";
    assert_no_rule(&scan("py", no_else), "BCS016");

    let impure = "def run(mode: object, h: object) -> int:\n    if next_mode() == 1:\n        h.check()\n        return 1\n    else:\n        h.check()\n        return 2\n";
    assert_no_rule(&scan("py", impure), "BCS016");

    let distinct = "def run(mode: int, h: object) -> int:\n    if mode == 1:\n        h.start()\n        return 1\n    else:\n        h.stop()\n        return 2\n";
    assert_no_rule(&scan("py", distinct), "BCS016");
}

#[test]
fn python_fstring_in_loop_does_not_trigger_bcs007_and_unused_allow_fails() {
    let source = "# bits:allow(BCS007): Python has no brace blocks\ndef show(items: list[int]) -> None:\n    for item in items:\n        print(f\"{item}\")\n";
    let results = scan("py", source);
    assert_no_rule(&results, "BCS007");
    assert!(results.iter().any(|r| {
        r["rule_id"] == "BCS015"
            && r["message"] == "unused bits:allow must be deleted"
            && r["severity"] == "violation"
    }));
}

#[test]
fn bits_prove_marks_warnings_for_critic_and_rejects_hard_violations() {
    let directory = tempfile::tempdir().unwrap();
    let long_fn = directory.path().join("long.py");
    let body = "    cursor += 1\n".repeat(49);
    fs::write(
        &long_fn,
        format!(
            "# bits:prove(BCS003, test=test_lock): shared lock; split unlocks\ndef step() -> int:\n    cursor = 0\n{body}    return cursor\n"
        ),
    )
    .unwrap();
    let output = bits(&[
        "style",
        "check",
        long_fn.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let report = json(&output);
    assert_eq!(report["summary"]["violations"], 0);
    assert_eq!(report["summary"]["proves"], 1);
    assert_eq!(report["results"][0]["obligation"], "critic_review");
    assert!(
        report["results"][0]["explanation"]
            .as_str()
            .unwrap()
            .contains("[test=test_lock]")
    );

    let nested = "// bits:prove(BCS003): cohesive; splitting risks state\nfunction bad(value) {\n  if (value) {\n    for (const item of value) {\n      while (item) {\n        if (item) {\n          return 1;\n        }\n      }\n    }\n  }\n  return 0;\n}\n";
    let results = scan("js", nested);
    assert!(results.iter().any(|r| {
        r["rule_id"] == "BCS015" && r["message"] == "bits:prove cannot waive a hard violation"
    }));
    assert!(
        results
            .iter()
            .any(|r| { r["rule_id"] == "BCS003" && r["severity"] == "violation" })
    );

    let malformed = "// bits:prove(BCS010): single part only\nfunction ok() {\n  return 1;\n}\n";
    assert!(scan("js", malformed).iter().any(|r| {
        r["rule_id"] == "BCS015"
            && r["message"] == "proof must state minimal form and concrete risk in two parts"
    }));

    let unused = "// bits:prove(BCS010): legacy name; framework requires it\nfunction ok() {\n  return 1;\n}\n";
    assert!(scan("js", unused).iter().any(|r| {
        r["rule_id"] == "BCS015" && r["message"] == "unused bits:prove must be deleted"
    }));
}

#[test]
fn diff_mode_enforces_single_new_prove_budget() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("README.md"), "# demo\n").unwrap();
    init_diff_repo(root);
    let body = "    value += 1\n".repeat(49);
    let make_fn = |name: &str| {
        format!(
            "# bits:prove(BCS003): atomic steps; split duplicates guards\ndef {name}() -> int:\n    value = 0\n{body}    return value\n"
        )
    };
    fs::write(root.join("one.py"), make_fn("one")).unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(0), "{:#?}", json(&output));

    fs::write(root.join("two.py"), make_fn("two")).unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report = json(&output);
    assert!(report["results"].as_array().unwrap().iter().any(|r| {
        r["rule_id"] == "BCS015"
            && r["message"] == "new bits:prove annotations exceed the per-change budget of 1"
    }));
}

#[test]
fn defaulted_and_keyword_only_bools_are_not_boolean_traps() {
    let options = "def fetch(url: str, *, follow_redirects: bool = False, trust_env: bool = True) -> str:\n    if follow_redirects and trust_env:\n        return url\n    return url\n";
    assert_no_rule(&scan("py", options), "BCS004");

    let positional = "def run(enabled: bool) -> bool:\n    return enabled\n";
    assert!(
        scan("py", positional)
            .iter()
            .any(|r| { r["rule_id"] == "BCS004" && r["severity"] == "violation" })
    );
}

#[test]
fn python_qualified_type_aliases_and_module_logger_are_not_mutable_globals() {
    let source = "import logging\nimport typing\n\nlogger = logging.getLogger(\"httpx\")\nEventHook = typing.Callable[..., typing.Any]\n";
    assert_no_rule(&scan("py", source), "BCS011");
}

#[test]
fn python_stdlib_shadow_and_type_checking_imports_do_not_create_cycles() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("pkg");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("__init__.py"), "").unwrap();
    fs::write(
        root.join("abc.py"),
        "from abc import ABC\n\n\nclass Base(ABC):\n    pass\n",
    )
    .unwrap();
    fs::write(
        root.join("a.py"),
        "from .b import B\n\n\nclass A:\n    def make(self) -> B:\n        return B()\n",
    )
    .unwrap();
    fs::write(
        root.join("b.py"),
        "from typing import TYPE_CHECKING\n\nif TYPE_CHECKING:\n    from .a import A\n    from .abc import Base\n\n\nclass B:\n    def base(self, a: \"A\") -> \"Base\":\n        return a\n",
    )
    .unwrap();
    let results = scan(
        "py",
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from .console import Console\n\n\ndef show(c: \"Console\") -> None:\n    pass\n",
    );
    assert_no_rule(&results, "BCS014");
    let report = scan_dir(root.parent().unwrap());
    assert_no_rule(report["results"].as_array().unwrap(), "BCS014");
}

#[test]
fn python_main_guard_typed_constants_and_optional_import_are_not_globals() {
    let source = "from typing import Literal\n\nUNSET: Literal[0] = 0\ntry:\n    import brotli\nexcept ImportError:\n    brotli = None\n\nif __name__ == \"__main__\":\n    console = object()\n    print(console)\n";
    assert_no_rule(&scan("py", source), "BCS011");
}

#[test]
fn bcs017_detects_direct_indirect_cross_file_and_custom_io_taint() {
    let direct =
        "# bits:spec(pure)\ndef size(path: str) -> int:\n    return len(open(path).read())\n";
    let results = scan("py", direct);
    assert!(results.iter().any(|r| {
        r["rule_id"] == "BCS017"
            && r["severity"] == "violation"
            && r["category"] == "role"
            && r["confidence"] == "high"
    }));

    let indirect = "def helper(path: str) -> str:\n    return open(path).read()\n\n\n# bits:spec(pure)\ndef compute(path: str) -> int:\n    return len(helper(path))\n";
    assert!(
        scan("py", indirect)
            .iter()
            .any(|r| r["rule_id"] == "BCS017" && r["range"]["start"]["line"] == 6)
    );

    let alias = "import os as o\n\n\n# bits:spec(pure)\ndef home() -> str:\n    return o.getenv(\"HOME\")\n";
    assert!(scan("py", alias).iter().any(|r| r["rule_id"] == "BCS017"));

    let safe = "def helper(data: dict) -> int:\n    return data.get(\"k\", 0)\n\n\n# bits:spec(pure)\ndef compute(data: dict) -> int:\n    return helper(data) + 1\n\n\ndef unmarked(path: str) -> str:\n    return open(path).read()\n";
    assert_no_rule(&scan("py", safe), "BCS017");

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(
        root.join("util.py"),
        "def read_cfg(path: str) -> str:\n    return open(path).read()\n",
    )
    .unwrap();
    fs::write(
        root.join("calc.py"),
        "from util import read_cfg\n\n\n# bits:spec(pure)\ndef calc(path: str) -> int:\n    return len(read_cfg(path))\n",
    )
    .unwrap();
    let report = scan_dir(root);
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| { r["rule_id"] == "BCS017" && r["file"] == "calc.py" })
    );

    fs::write(root.join("bits.toml"), "io_sources = [\"db.query\"]\n").unwrap();
    fs::write(
        root.join("calc.py"),
        "# bits:spec(pure)\ndef calc(db: object) -> int:\n    return db.query()\n",
    )
    .unwrap();
    let report = scan_dir(root);
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| { r["rule_id"] == "BCS017" && r["actual"] == "db.query" })
    );
}

#[test]
fn improving_ratchet_rewards_reduction_but_blocks_regressions_and_new_functions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let file = root.join("legacy.py");
    let nested_fn = |worse: bool| {
        let loop_line = if worse {
            "                while item > 0:\n                    if item == 3:\n                        item += 1\n                    item -= 1\n"
        } else {
            "                if item == 3:\n                    item += 3\n"
        };
        format!(
            "def legacy(value: int) -> int:\n    if value > 0:\n        for item in range(value):\n            if item % 2 == 0:\n{loop_line}    if value == 1:\n        value += 1\n    if value == 2:\n        value += 1\n    if value == 3:\n        value += 1\n    if value == 4:\n        value += 1\n    if value == 5:\n        value += 1\n    if value == 6:\n        value += 1\n    if value == 7:\n        value += 1\n    if value == 8:\n        value += 1\n    if value == 9:\n        value += 1\n    return value\n"
        )
    };
    fs::write(&file, nested_fn(true)).unwrap();
    init_diff_repo(root);

    // Reduce nesting from 5 to 4 and remove one branch: still over threshold, but improving.
    fs::write(&file, nested_fn(false)).unwrap();
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
    assert!(report["summary"]["improving"].as_u64().unwrap() >= 1);
    assert!(report["results"].as_array().unwrap().iter().any(|r| {
        r["rule_id"] == "BCS003"
            && r["improving"] == true
            && r["severity"] == "warning"
            && r["obligation"] == "ratchet_improving"
    }));

    // Add a new deeply nested function: new functions get no ratchet.
    let added = format!(
        "{}\n{}",
        nested_fn(false),
        nested_fn(false).replace("def legacy(", "def brand_new(")
    );
    fs::write(&file, added).unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));

    // Worsen complexity beyond HEAD: ratchet must not apply.
    let worsened = nested_fn(true).replace(
        "    return value\n",
        "    if value == 10:\n        value += 10\n    return value\n",
    );
    fs::write(&file, worsened).unwrap();
    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--format",
        "json",
    ]);
    assert_eq!(output.status.code(), Some(1));
}

fn scan_dir(root: &Path) -> Value {
    let mut args = vec![
        "style",
        "report",
        root.to_str().unwrap(),
        "--format",
        "json",
    ];
    let config = root.join("bits.toml");
    if config.is_file() {
        args.push("--config");
        args.push(config.to_str().unwrap());
    }
    let output = bits(&args);
    assert_eq!(output.status.code(), Some(0));
    json(&output)
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
