use std::fs;
use std::process::Command;

use serde_json::Value;

fn scan(extension: &str, source: &str) -> Vec<Value> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(format!("sample.{extension}"));
    fs::write(&path, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_bits"))
        .args([
            "style",
            "report",
            path.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["summary"]["scan_errors"], 0, "{report:#}");
    report["results"].as_array().unwrap().clone()
}

fn assert_rule(results: &[Value], rule: &str) {
    assert!(
        results.iter().any(|result| result["rule_id"] == rule),
        "missing {rule}: {results:#?}"
    );
}

#[test]
fn bcs002_allows_only_long_user_visible_strings() {
    let long_string =
        format!("def message():\n\treturn \"{}\"\n", "a".repeat(100));
    assert!(
        !scan("py", &long_string)
            .iter()
            .any(|result| result["rule_id"] == "BCS002")
    );

    let long_code = format!("def message():\n\treturn {}\n", "a".repeat(100));
    assert_rule(&scan("py", &long_code), "BCS002");
}

#[test]
fn bcs003_checks_function_lines_nesting_locals_and_complexity() {
    let long_function =
        format!("def long_function():\n{}", "\tvalue = 1\n".repeat(49));
    let results = scan("py", &long_function);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS003"
        && result["message"] == "function exceeds 48 code lines"));

    let nested = "def nested(value):\n\tif value:\n\t\tfor item in value:\n\t\t\twhile item:\n\t\t\t\tif item:\n\t\t\t\t\treturn 1\n\treturn 0\n";
    let results = scan("py", nested);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS003"
        && result["message"] == "control-flow nesting exceeds three levels"));

    let locals = format!(
        "def many_locals():\n{}",
        (0..11)
            .map(|index| format!("\tvalue_{index} = {index}\n"))
            .collect::<String>()
    );
    let results = scan("py", &locals);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS003"
        && result["message"] == "function has more than ten local variables"));

    let complex = format!(
        "def complex(value):\n{}\treturn value\n",
        "\tif value:\n\t\tvalue += 1\n".repeat(10)
    );
    let results = scan("py", &complex);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS003"
        && result["message"] == "cyclomatic complexity exceeds ten"));
}

#[test]
fn bcs005_rejects_backward_goto() {
    let source = "int update(int value)\n{\nout_done:\n\treturn value;\n\tgoto out_done;\n}\n";
    let results = scan("c", source);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS005"
        && result["actual"] == "backward_jump"));
}

#[test]
fn bcs006_rejects_condition_and_chained_assignments() {
    let source = "int update(int left, int right)\n{\n\tif (left = right)\n\t\treturn 1;\n\tleft = right = 0;\n\treturn left;\n}\n";
    let results = scan("c", source);
    assert!(
        results
            .iter()
            .filter(|result| result["rule_id"] == "BCS006")
            .count()
            >= 2
    );
}

#[test]
fn bcs006_rejects_multiple_statements_on_one_line() {
    let source = "export function update(left, right)\n{\n\tlet first = 0; let second = 0;\n\treturn first + second + left + right;\n}\n";
    assert_rule(&scan("js", source), "BCS006");
}

#[test]
fn bcs007_requires_braces_and_profile_placement() {
    let unbraced = "function choose(value) {\n  if (value)\n    return 1;\n  return 0;\n}\n";
    assert_rule(&scan("js", unbraced), "BCS007");

    let consistent = "function choose(value) {\n  if (value) {\n    return 1;\n  } else {\n    return 0;\n  }\n}\n";
    assert!(
        !scan("js", consistent)
            .iter()
            .any(|result| result["rule_id"] == "BCS007")
    );
}

#[test]
fn bcs008_rejects_double_indented_case() {
    let source = "int classify(int value)\n{\n\tswitch (value) {\n\t\tcase 1:\n\t\t\treturn 1;\n\tdefault:\n\t\treturn 0;\n\t}\n}\n";
    assert_rule(&scan("c", source), "BCS008");
}

#[test]
fn bcs010_rejects_non_pascal_type_names() {
    assert_rule(&scan("py", "class bad_name:\n\tpass\n"), "BCS010");
}

#[test]
fn bcs011_rejects_implicit_environment_input() {
    let source =
        "import os\n\ndef read_value():\n\treturn os.getenv(\"HOME\")\n";
    assert_rule(&scan("py", source), "BCS011");
}

#[test]
fn bcs012_rejects_conditional_compilation_inside_functions() {
    let source = "int choose(int value)\n{\n#ifdef FEATURE\n\treturn value;\n#else\n\treturn 0;\n#endif\n}\n";
    assert_rule(&scan("c", source), "BCS012");
}

#[test]
fn bcs013_rejects_empty_catch_and_forged_success() {
    let empty = "def parse(value):\n\ttry:\n\t\treturn int(value)\n\texcept ValueError:\n\t\tpass\n";
    assert_rule(&scan("py", empty), "BCS013");

    let forged = "function parse(value)\n{\n\ttry {\n\t\treturn Number(value);\n\t} catch (error) {\n\t\treturn true;\n\t}\n}\n";
    assert_rule(&scan("js", forged), "BCS013");

    let cpp = "int parse(int value)\n{\n\ttry {\n\t\treturn value;\n\t} catch (...) {\n\t}\n}\n";
    assert_rule(&scan("cpp", cpp), "BCS013");
}

#[test]
fn bcs014_rejects_wildcard_imports_and_unreachable_code() {
    assert_rule(&scan("py", "from os.path import *\n"), "BCS014");

    let unreachable = "def choose(value):\n\treturn value\n\tprint(value)\n";
    assert_rule(&scan("py", unreachable), "BCS014");
}

#[test]
fn bcs015_rejects_editor_modelines() {
    let source = "# vim:set sw=4 noet\ndef valid():\n\treturn 1\n";
    assert_rule(&scan("py", source), "BCS015");
}

#[test]
fn bcs003_ignores_comments_and_flat_switch_cases() {
    let comments = format!(
        "def documented():\n{}\treturn 1\n",
        "\t# explanation\n".repeat(49)
    );
    assert!(
        !scan("py", &comments)
            .iter()
            .any(|result| result["rule_id"] == "BCS003")
    );

    let cases = (1..=12)
        .map(|value| format!("\tcase {value}:\n\t\treturn {value};\n"))
        .collect::<String>();
    let source = format!(
        "int classify(int value)\n{{\n\tswitch (value) {{\n{cases}\tdefault:\n\t\treturn 0;\n\t}}\n}}\n"
    );
    assert!(
        !scan("c", &source)
            .iter()
            .any(|result| result["rule_id"] == "BCS003")
    );
}

#[test]
fn bcs006_rejects_nested_ternary_and_double_negation() {
    let nested = "function choose(first, second, third, fourth) {\n  return first ? second : third ? fourth : 0;\n}\n";
    assert_rule(&scan("js", nested), "BCS006");

    let negation = "function choose(value) {\n  return !!value;\n}\n";
    assert_rule(&scan("js", negation), "BCS006");
}

#[test]
fn bcs011_rejects_parameter_reassignment() {
    let source = "def update(value):\n    value = 1\n    return value\n";
    assert_rule(&scan("py", source), "BCS011");
}

#[test]
fn bcs013_rejects_unrestricted_catch_all() {
    let source = "def parse(value):\n    try:\n        return int(value)\n    except Exception:\n        return 0\n";
    assert_rule(&scan("py", source), "BCS013");
}
