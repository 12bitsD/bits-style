use std::fs;
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

fn assert_empty(results: &[Value]) {
    assert!(results.is_empty(), "unexpected results: {results:#?}");
}

#[test]
fn prettier_operators_conditions_and_calls_have_no_false_positives() {
    let source = "function compare(firstCondition, secondCondition) {\n  if (\n    firstCondition &&\n    secondCondition\n  ) {\n    return firstCondition === secondCondition;\n  }\n  return invoke(\n    firstCondition,\n    secondCondition,\n  );\n}\n";
    assert_empty(&scan("js", source));
}

#[test]
fn chinese_business_comments_are_not_commented_code() {
    let source = "// 这里包含分号、括号和花括号，用于解释业务约束；不是旧实现。\nfunction update() {\n  return 1;\n}\n";
    assert_empty(&scan("js", source));

    let actual = "// return value;\nfunction update() {\n  return 1;\n}\n";
    let results = scan("js", actual);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS014" && result["severity"] == "warning"
    }));
}

#[test]
fn type_only_and_mixed_imports_are_counted_by_binding() {
    let source = r#"import type { AppSettings, CommandItem } from "./types";
import React, { useState } from "react";

export function view(settings: AppSettings) {
  const [item] = useState<CommandItem>({ id: "1" });
  void React;
  return <span>{settings.name}{item.id}</span>;
}
"#;
    assert_empty(&scan("tsx", source));
}

#[test]
fn module_constants_and_local_const_have_distinct_naming() {
    let source = "const MAX_COUNT = 2;\nexport const api = { value: 1 };\n\nexport function update() {\n  const localValue = 1;\n  return localValue + MAX_COUNT + api.value;\n}\n";
    assert_empty(&scan("js", source));
}

#[test]
fn boolean_object_fields_are_not_behavior_flags() {
    let source = "function finish({ retryable }: { retryable: boolean }) {\n  return !retryable;\n}\n\nfunction run(enabled: boolean) {\n  return enabled;\n}\n";
    let results = scan("ts", source);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["rule_id"], "BCS004");
    assert_eq!(results[0]["actual"], "enabled: boolean");
}

#[test]
fn suppression_covers_the_whole_function_signature() {
    let source = "// bits:allow(BCS004): existing framework signature\nfunction getArtifactVersion(\n  first: number,\n  second: number,\n  third: number,\n  fourth: number,\n  fifth: number,\n) {\n  return first + second + third + fourth + fifth;\n}\n";
    assert_empty(&scan("ts", source));
}

#[test]
fn multiline_type_declarations_do_not_count_as_function_implementation() {
    let fields = (1..=50)
        .map(|index| format!("    field{index}: string;\n"))
        .collect::<String>();
    let source = format!(
        "function render() {{\n  type Props = {{\n{fields}  }};\n  return {{ value: \"ok\" }};\n}}\n"
    );
    assert_empty(&scan("ts", &source));
}

#[test]
fn worker_object_state_updates_are_not_parameter_reassignment() {
    let source = "function update(context) {\n  context.closed = true;\n  context.stdout += \"chunk\";\n  return context;\n}\n";
    assert_empty(&scan("js", source));

    let reassignment =
        "function replace(context) {\n  context = {};\n  return context;\n}\n";
    let results = scan("js", reassignment);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS011"));
}

#[test]
fn expected_failure_sentinels_are_allowed_while_forged_success_warns() {
    let parse = "function parse(value) {\n  try {\n    return JSON.parse(value);\n  } catch (error) {\n    return null;\n  }\n}\n";
    assert!(
        !scan("js", parse)
            .iter()
            .any(|result| result["rule_id"] == "BCS013")
    );

    let guard = "function guard(value) {\n  try {\n    return validate(value);\n  } catch (error) {\n    return false;\n  }\n}\n";
    assert!(
        !scan("js", guard)
            .iter()
            .any(|result| result["rule_id"] == "BCS013")
    );

    let forged = "function verify(value) {\n  try {\n    return validate(value);\n  } catch (error) {\n    return true;\n  }\n}\n";
    let results = scan("js", forged);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS013" && result["severity"] == "warning"
    }));
}

#[test]
fn empty_catch_remains_a_hard_error() {
    let source = "function parse(value) {\n  try {\n    return JSON.parse(value);\n  } catch (error) {\n  }\n}\n";
    let results = scan("js", source);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS013" && result["severity"] == "violation"
    }));
}

#[test]
fn nesting_and_cyclomatic_complexity_are_hard_errors() {
    let nested = "function nested(value) {\n  if (value) {\n    for (const item of value) {\n      while (item) {\n        if (item) {\n          return 1;\n        }\n      }\n    }\n  }\n  return 0;\n}\n";
    let results = scan("js", nested);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS003" && result["severity"] == "violation"
    }));

    let complex = format!(
        "function complex(value) {{\n{}  return value;\n}}\n",
        "  if (value) {\n    value += 1;\n  }\n".repeat(10)
    );
    let results = scan("js", &complex);
    assert!(results.iter().any(|result| {
        result["rule_id"] == "BCS003" && result["severity"] == "violation"
    }));
}
