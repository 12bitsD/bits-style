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
fn keyword_arguments_are_not_chained_assignments() {
    let source = "from pydantic import Field\n\nclass Example:\n    name: str = Field(min_length=1, max_length=30)\n    value = dict(exclude_none=True)\n";
    assert_empty(&scan("py", source));

    let chained = "a = b = 1\n";
    let results = scan("py", chained);
    assert!(results.iter().any(|result| result["rule_id"] == "BCS006"));
}

#[test]
fn python_call_keywords_are_not_function_calls() {
    let source = "import os\nvalue = (\n  1 and\n  2 or\n  3\n)\nassert value in (1, 2)\nassert not (value is None)\nif value:\n    pass\nelse:\n    pass\ntry:\n    pass\nexcept (ValueError):\n    pass\n";
    assert!(
        !scan("py", source)
            .iter()
            .any(|result| result["rule_id"] == "BCS009")
    );

    let delete = "function remove(value) {\n  return delete (value);\n}\n";
    assert!(
        !scan("js", delete)
            .iter()
            .any(|result| result["rule_id"] == "BCS009")
    );
}

#[test]
fn imports_used_in_template_expressions_count_as_references() {
    let python =
        "from uuid import uuid4\n\ndef make_id():\n    return f\"{uuid4()}\"\n";
    assert_empty(&scan("py", python));

    let javascript = "import { randomUUID } from \"crypto\";\n\nexport function makeId() {\n  return `${randomUUID()}`;\n}\n";
    assert_empty(&scan("js", javascript));
}

#[test]
fn numeric_constants_and_constant_expressions_are_supported() {
    let source = "MAX_COUNT = 300_000\nMAX_SIZE = 1024 * 1024\n";
    assert_empty(&scan("py", source));
}

#[test]
fn type_aliases_are_not_mutable_globals_and_use_type_names() {
    let source = "ProviderFactory = Callable[..., ModelProvider]\nSafeInt = Annotated[int, Field(gt=0)]\n_ObservedProvider = Literal[\"x\"]\n";
    assert_empty(&scan("py", source));
}

#[test]
fn pydantic_and_json_fields_keep_external_names() {
    let source = "class ExperimentConfig(StrictModel):\n    directionCount: int | None = Field(default=None)\n    topP: float | None = Field(default=None)\n";
    assert_empty(&scan("py", source));
}

#[test]
fn annotated_variables_are_not_type_aliases() {
    let source = "def run():\n    recorder: StepRecorder | None = None\n    parsed: StepExperimentRequest | None = None\n";
    assert_empty(&scan("py", source));
}

#[test]
fn reexport_names_count_as_import_references() {
    let source = "from ..errors import AgentFileUnavailableError\nfrom .skill_files import read_agent_file\n\n__all__ = [\"AgentFileUnavailableError\", \"read_agent_file\"]\n";
    assert_empty(&scan("py", source));
}

#[test]
fn external_fields_and_framework_callbacks_are_not_renamed() {
    let source = "class Handler(BaseHTTPRequestHandler):\n    def do_POST(self) -> None:\n        pass\n\ndef update(reused):\n    reused.content.world[1].itemId = \"world-1\"\n    reused.content.writingPlan[0].itemId = \"plan-1\"\n";
    assert_empty(&scan("py", source));
}

#[test]
fn constants_accept_literal_and_module_naming_styles() {
    let source = "const storageKey = \"a4n-theme\";\nconst MAX_COUNT = 2;\n";
    assert_empty(&scan("js", source));
}

#[test]
fn memo_wrapped_react_components_are_recognized() {
    let source = "export const FiniteMarkdown = memo(function FiniteMarkdown({ source }) {\n  return <div>{source}</div>;\n});\n";
    assert_empty(&scan("tsx", source));
}

#[test]
fn dynamic_import_generics_and_type_references_parse() {
    let directory = tempfile::tempdir().unwrap();
    let generic = directory.path().join("generic.ts");
    fs::write(&generic, "vi.mock('x', async importOriginal => ({ ...await importOriginal<typeof import('../x')>(), value: 1 }))\n").unwrap();
    let output = bits(&[
        "style",
        "report",
        generic.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(json(&output)["summary"]["scan_errors"], 0);

    let types = directory.path().join("types.ts");
    fs::write(&types, "const entry: import('../src/pipeline/pipeline.js').PipelineDefinitionEntry = value\nconst input: import('../src/pipeline/pipeline.js').PipelineInput = input\n").unwrap();
    let output = bits(&[
        "style",
        "report",
        types.to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(json(&output)["summary"]["scan_errors"], 0);
}

#[test]
fn multiline_imports_do_not_create_empty_bindings() {
    let source = "from ..domain.work import (\n    AgentConfig,\n    BeatContent,\n    PreviousChapter,\n    ProseContent,\n    ProseEditDraft,\n    SettingContent,\n)\n\ndef use_all():\n    return (\n        AgentConfig,\n        BeatContent,\n        PreviousChapter,\n        ProseContent,\n        ProseEditDraft,\n        SettingContent,\n    )\n";
    assert_empty(&scan("py", source));
}

#[test]
fn rust_unwrap_and_panic_are_delegated_to_clippy() {
    let results = scan(
        "rs",
        "fn read(value: Result<i32, Error>) -> i32 {\n    value.expect(\"valid\")\n}\n\nfn fail() -> ! {\n    panic!(\"boom\")\n}\n",
    );
    assert!(!results.iter().any(|result| result["rule_id"] == "BCS013"));
}

#[test]
fn rust_match_arms_follow_profile_indentation() {
    let source = "fn classify(value: i32) -> i32 {\n    match value {\n    1 => 1,\n    _ => 0,\n    }\n}\n";
    let results = scan("rs", source);
    assert!(!results.iter().any(|result| result["rule_id"] == "BCS008"));
}

#[test]
fn rust_trait_imports_are_not_unused_by_identifier_count() {
    let source = "use std::io::Write;\n\nfn write_all<W: Write>(writer: &mut W, bytes: &[u8]) {\n    writer.write_all(bytes).unwrap();\n}\n";
    let results = scan("rs", source);
    assert!(
        !results.iter().any(|result| result["rule_id"] == "BCS014"
            && result["actual"] == "Write")
    );
}

#[test]
fn rust_pub_use_is_a_reexport() {
    let source = "pub use crate::foo;\n";
    assert_empty(&scan("rs", source));
}
