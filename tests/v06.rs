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
    let output = bits(&["style", "report", path.to_str().unwrap(), "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)["results"].as_array().unwrap().clone()
}

fn run_git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

fn init_repo(root: &Path) {
    run_git(root, &["init", "-q"]);
    run_git(root, &["config", "user.email", "bits@example.invalid"]);
    run_git(root, &["config", "user.name", "Bits Tests"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-qm", "initial"]);
}

#[test]
fn diff_skips_unsupported_extensions_by_default_and_honors_strict_flag() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("app.py"), "def ok() -> int:\n    return 1\n").unwrap();
    init_repo(root);
    fs::write(root.join("deploy.sh"), "#!/usr/bin/env bash\necho ok\n")
        .unwrap();
    fs::write(root.join("app.py"), "def ok() -> int:\n    return 2\n").unwrap();

    let default_diff =
        bits(&["style", "check", root.to_str().unwrap(), "--diff", "--json"]);
    assert_eq!(default_diff.status.code(), Some(0));
    let report = json(&default_diff);
    assert_eq!(report["summary"]["unsupported_files"], 1);
    assert_eq!(report["unsupported"], serde_json::json!(["deploy.sh"]));

    let strict_diff = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--diff",
        "--strict-unsupported",
    ]);
    assert_eq!(strict_diff.status.code(), Some(2));
}

#[test]
fn bcs002_stays_violation_and_exempts_multiline_strings_and_urls() {
    let multiline_doc = format!(
        "def doc() -> str:\n    \"\"\"\n    {}\n    \"\"\"\n    return \"ok\"\n",
        "word ".repeat(25)
    );
    assert!(
        !scan("py", &multiline_doc)
            .iter()
            .any(|r| r["rule_id"] == "BCS002")
    );

    let template_svg = format!(
        "export function icon() {{\n  return `<svg>\n    <path d=\"{}\" />\n  </svg>`;\n}}\n",
        "M0 0 L10 10 ".repeat(12)
    );
    assert!(
        !scan("js", &template_svg)
            .iter()
            .any(|r| r["rule_id"] == "BCS002")
    );

    let url_comment = format!(
        "# See https://example.com/{}\ndef ok() -> int:\n    return 1\n",
        "segment/".repeat(12)
    );
    assert!(
        !scan("py", &url_comment)
            .iter()
            .any(|r| r["rule_id"] == "BCS002")
    );

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("wide.py");
    let wide_code = format!(
        "def wide() -> int:\n    return {}\n",
        "1 + ".repeat(25) + "0"
    );
    fs::write(&path, wide_code).unwrap();
    let output = bits(&["style", "check", path.to_str().unwrap(), "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let report = json(&output);
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rule_id"] == "BCS002" && r["severity"] == "violation")
    );
}

#[test]
fn bcs003_keeps_strict_line_and_local_limits_everywhere_and_attaches_hotspots()
{
    let directory = tempfile::tempdir().unwrap();
    let test_file = directory.path().join("test_flow.py");
    let linear_steps = (0..55)
        .map(|i| format!("    step_{i} = {i}\n"))
        .collect::<String>();
    fs::write(
        &test_file,
        format!("def test_end_to_end() -> None:\n{linear_steps}    assert step_0 == 0\n"),
    )
    .unwrap();
    let report = json(&bits(&[
        "style",
        "check",
        test_file.to_str().unwrap(),
        "--json",
    ]));
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rule_id"] == "BCS003"
                && r["message"] == "function exceeds 48 code lines"),
        "{report:#}"
    );
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rule_id"] == "BCS003"
                && r["message"]
                    == "function has more than ten local variables"),
        "{report:#}"
    );

    let nested_test = directory.path().join("test_nested.py");
    fs::write(
        &nested_test,
        "def test_bad(items: list[int]) -> None:\n    if items:\n        for x in items:\n            if x > 0:\n                if x < 10:\n                    assert x\n",
    )
    .unwrap();
    let nested_out =
        bits(&["style", "check", nested_test.to_str().unwrap(), "--json"]);
    assert_eq!(nested_out.status.code(), Some(1));
    let nested_report = json(&nested_out);
    let bcs003 = nested_report["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule_id"] == "BCS003" && r["severity"] == "violation")
        .expect("must flag deep nesting in test");
    assert!(!bcs003["hotspots"].as_array().unwrap().is_empty());

    let js_consts = format!(
        "export function render(item) {{\n{}  return item;\n}}\n",
        (0..12)
            .map(|i| format!("  const label{i} = item.v{i};\n"))
            .collect::<String>()
    );
    assert!(
        scan("js", &js_consts)
            .iter()
            .any(|r| r["rule_id"] == "BCS003"
                && r["message"]
                    == "function has more than ten local variables")
    );
}

#[test]
fn bcs003_warns_on_non_test_files_over_300_code_lines() {
    let directory = tempfile::tempdir().unwrap();
    let big_src = directory.path().join("huge.py");
    let funcs = (0..110)
        .map(|i| {
            format!(
                "def fn_{i}(x: int) -> int:\n    y = x + {i}\n    return y\n\n"
            )
        })
        .collect::<String>();
    fs::write(&big_src, &funcs).unwrap();
    let report = json(&bits(&[
        "style",
        "check",
        big_src.to_str().unwrap(),
        "--json",
    ]));
    assert!(
        report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rule_id"] == "BCS003"
                && r["message"] == "file exceeds 300 code lines"
                && r["severity"] == "warning")
    );

    let big_test = directory.path().join("test_huge.py");
    fs::write(&big_test, &funcs).unwrap();
    let test_report = json(&bits(&[
        "style",
        "check",
        big_test.to_str().unwrap(),
        "--json",
    ]));
    assert!(
        !test_report["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["message"] == "file exceeds 300 code lines")
    );
}

#[test]
fn bcs004_excludes_python_self_cls_star_and_calibrates_di_constructors() {
    let method_ok = "class Svc:\n    def emit(self, *, a: int, b: int, c: int, d: int) -> int:\n        return a + b + c + d\n\n    @classmethod\n    def build(cls, *, a: int, b: int, c: int) -> int:\n        return a + b + c\n\n    def __init__(self, *, a: int, b: int, c: int, d: int, e: int, f: int) -> None:\n        self.total = a + b + c + d + e + f\n";
    assert!(
        !scan("py", method_ok)
            .iter()
            .any(|r| r["rule_id"] == "BCS004")
    );

    let too_many_kw = "def lint_doc(*, a: int, b: int, c: int, d: int, e: int, f: int, g: int, h: int, i: int) -> int:\n    return a + b + c + d + e + f + g + h + i\n";
    assert!(
        scan("py", too_many_kw)
            .iter()
            .any(|r| r["rule_id"] == "BCS004" && r["severity"] == "warning")
    );
}

#[test]
fn bcs006_distinguishes_mutable_and_scalar_chained_assignments() {
    let scalar = "def bounds(base: int) -> tuple[int, int]:\n    lo = hi = base\n    return (lo, hi)\n";
    let scalar_res = scan("py", scalar);
    assert!(
        scalar_res
            .iter()
            .any(|r| r["rule_id"] == "BCS006" && r["severity"] == "warning")
    );

    let mutable = "def shared() -> tuple[list[int], list[int]]:\n    a = b = []\n    return (a, b)\n";
    let mutable_res = scan("py", mutable);
    assert!(
        mutable_res
            .iter()
            .any(|r| r["rule_id"] == "BCS006" && r["severity"] == "violation")
    );
}

#[test]
fn bcs011_exempts_composition_root_and_injected_env_fallback() {
    let source = "import os\n\n\ndef main() -> str:\n    return os.getenv(\"HOME\", \"\")\n\n\ndef load_cfg(env: dict[str, str] | None = None) -> str:\n    src = os.environ if env is None else env\n    return src.get(\"APP\", \"\")\n\n\nif __name__ == \"__main__\":\n    _ = os.getenv(\"HOME\", \"\")\n";
    assert!(!scan("py", source).iter().any(|r| r["rule_id"] == "BCS011"));

    let deep = "import os\n\n\ndef business_logic(key: str) -> str:\n    return os.getenv(key, \"\")\n";
    assert!(
        scan("py", deep)
            .iter()
            .any(|r| r["rule_id"] == "BCS011" && r["severity"] == "warning")
    );
}

#[test]
fn bcs013_allows_explicit_false_and_none_sentinels_and_flags_forged_ok_dict() {
    let clean_py = "def verify(raw: str) -> bool:\n    try:\n        int(raw)\n        return True\n    except ValueError:\n        return False\n\n\ndef parse_opt(raw: str) -> int | None:\n    try:\n        return int(raw)\n    except ValueError:\n        return None\n";
    assert!(
        !scan("py", clean_py)
            .iter()
            .any(|r| r["rule_id"] == "BCS013")
    );

    let forged_dict = "def write_item(raw: str) -> dict[str, bool]:\n    try:\n        int(raw)\n        return {\"ok\": True}\n    except ValueError:\n        return {\"ok\": True}\n";
    assert!(
        scan("py", forged_dict)
            .iter()
            .any(|r| r["rule_id"] == "BCS013" && r["severity"] == "warning")
    );
}

#[test]
fn agent_output_formats_deduplicate_hints_group_violations_bottom_and_support_markdown()
 {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("noisy.py");
    let long_a = format!(
        "def long_a() -> int:\n{}    return 1\n",
        "    x = 1\n".repeat(50)
    );
    let long_b = format!(
        "def long_b() -> int:\n{}    return 1\n",
        "    x = 1\n".repeat(50)
    );
    let bad = "def bad(x: int) -> int:\n    if x:\n        for i in range(x):\n            if i > 0:\n                if i < 5:\n                    return i\n    return 0\n";
    let proven = format!(
        "# bits:prove(BCS003): atomic lock; split breaks lock\ndef guarded() -> int:\n{}    return 1\n",
        "    x = 1\n".repeat(50)
    );
    fs::write(&path, format!("{long_a}\n{long_b}\n{bad}\n{proven}")).unwrap();

    let text_out = bits(&["style", "check", path.to_str().unwrap()]);
    assert_eq!(text_out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&text_out.stdout);
    assert_eq!(
        text.matches("register bits:prove(BCS003").count(),
        1,
        "{text}"
    );
    let warn_pos = text.find("severity=warning").unwrap();
    let viol_pos = text.find("Violations (blocking):").unwrap();
    assert!(warn_pos < viol_pos, "{text}");
    assert!(text.contains("hotspots:"), "{text}");

    let compact_out = bits(&[
        "style",
        "check",
        path.to_str().unwrap(),
        "--format",
        "compact",
    ]);
    assert_eq!(compact_out.status.code(), Some(1));
    let compact = String::from_utf8_lossy(&compact_out.stdout);
    assert!(compact.contains("[W] "));
    assert!(compact.contains("--- violations ---"));
    assert!(compact.contains("[V] "));
    assert!(!compact.contains("hint: prefer refactoring"));

    let vo_out = bits(&[
        "style",
        "check",
        path.to_str().unwrap(),
        "--violations-only",
        "--format",
        "compact",
    ]);
    assert_eq!(vo_out.status.code(), Some(1));
    let vo = String::from_utf8_lossy(&vo_out.stdout);
    assert!(!vo.contains("[W] "), "{vo}");
    assert!(vo.contains("[V] "), "{vo}");
    assert!(vo.contains("warnings=4"), "{vo}");

    let md_out = bits(&[
        "style",
        "report",
        path.to_str().unwrap(),
        "--format",
        "markdown",
    ]);
    assert_eq!(md_out.status.code(), Some(0));
    let md = String::from_utf8_lossy(&md_out.stdout);
    assert!(md.contains("## Bits Style Summary: FAIL"), "{md}");
    assert!(md.contains("### `bits:prove` Review Checklist"), "{md}");
    assert!(md.contains("atomic lock; split breaks lock"), "{md}");
}

#[test]
fn min_version_gate_and_baseline_stale_detection_and_prune_work() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let cfg_too_new = root.join("too-new.toml");
    fs::write(&cfg_too_new, "min_version = \"99.0.0\"\n").unwrap();
    fs::write(root.join("ok.py"), "def ok() -> int:\n    return 1\n").unwrap();
    let fail = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--config",
        cfg_too_new.to_str().unwrap(),
    ]);
    assert_eq!(fail.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&fail.stderr).contains("min_version"));

    let cfg_ok = root.join("ok.toml");
    fs::write(&cfg_ok, "min_version = \"0.5.0\"\n").unwrap();
    let pass = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--config",
        cfg_ok.to_str().unwrap(),
    ]);
    assert_eq!(pass.status.code(), Some(0));

    let keep = root.join("keep.py");
    let gone = root.join("gone.py");
    let baseline = root.join("bits-baseline.json");
    fs::write(&keep, "keep_global = 0\n").unwrap();
    fs::write(&gone, "gone_global = 0\n").unwrap();
    let generated = bits(&[
        "style",
        "report",
        root.to_str().unwrap(),
        "--output",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(generated.status.code(), Some(0));

    fs::remove_file(&gone).unwrap();
    let checked = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(checked.status.code(), Some(0));
    let report = json(&checked);
    assert_eq!(report["summary"]["suppressed_violations"], 1);
    assert_eq!(report["summary"]["stale_baseline_entries"], 1);

    let pruned = bits(&[
        "style",
        "baseline",
        "--prune",
        baseline.to_str().unwrap(),
        root.to_str().unwrap(),
    ]);
    assert_eq!(pruned.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&pruned.stdout)
            .contains("Pruned 1 stale baseline entry")
    );

    let after_prune = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--baseline",
        baseline.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(after_prune.status.code(), Some(0));
    assert_eq!(json(&after_prune)["summary"]["stale_baseline_entries"], 0);
}

#[test]
fn anti_bad_taste_rules_catch_parenthesized_ternary_nonlocal_multi_out_silent_catch_and_dead_private()
 {
    let paren_ternary = "def choose(a: bool, b: bool) -> str:\n    return \"x\" if a else (\"y\" if b else \"z\")\n";
    assert!(
        scan("py", paren_ternary)
            .iter()
            .any(|r| r["rule_id"] == "BCS006"
                && r["message"] == "nested ternary expressions are forbidden"
                && r["severity"] == "violation")
    );

    let nonlocal_fn = "def outer() -> int:\n    count = 0\n    def bump() -> None:\n        nonlocal count\n        count += 1\n    bump()\n    return count\n";
    assert!(
        scan("py", nonlocal_fn)
            .iter()
            .any(|r| r["rule_id"] == "BCS011"
                && r["message"]
                    == "implicit cross-scope state mutation via global/nonlocal is forbidden"
                && r["severity"] == "violation")
    );

    let multi_out = "def lint_doc(text: str, orphans: list[str], stale: list[str]) -> None:\n    if not text:\n        orphans.append(\"a\")\n    stale.append(\"b\")\n";
    assert!(
        scan("py", multi_out)
            .iter()
            .any(|r| r["rule_id"] == "BCS004"
                && r["message"]
                    == "function mutates multiple output container parameters instead of returning a value"
                && r["severity"] == "warning")
    );

    let silent_catch = "def dispatch(raw: str) -> None:\n    try:\n        int(raw)\n    except Exception:\n        return\n";
    assert!(
        scan("py", silent_catch)
            .iter()
            .any(|r| r["rule_id"] == "BCS013"
                && r["message"]
                    == "catch-all handler must not silently swallow errors with bare return/continue/break"
                && r["severity"] == "violation")
    );

    let dead_js = "const DEAD_FLAGS = ['a'];\nfunction orphanHelper() {\n  return 1;\n}\nexport function live() {\n  return 2;\n}\n";
    let dead_js_res = scan("js", dead_js);
    assert!(
        dead_js_res
            .iter()
            .any(|r| r["rule_id"] == "BCS014" && r["actual"] == "DEAD_FLAGS")
    );
    assert!(
        dead_js_res
            .iter()
            .any(|r| r["rule_id"] == "BCS014" && r["actual"] == "orphanHelper")
    );

    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(
        root.join("helpers.py"),
        "def _shared() -> int:\n    return 1\n\n\ndef _orphan() -> int:\n    return 2\n",
    )
    .unwrap();
    fs::write(
        root.join("caller.py"),
        "from helpers import _shared\n\n\ndef run() -> int:\n    return _shared()\n",
    )
    .unwrap();
    let dir_out = bits(&["style", "report", root.to_str().unwrap(), "--json"]);
    let dir_res = json(&dir_out)["results"].as_array().unwrap().clone();
    assert!(
        !dir_res
            .iter()
            .any(|r| r["rule_id"] == "BCS014" && r["actual"] == "_shared"),
        "{dir_res:#?}"
    );
    assert!(
        dir_res
            .iter()
            .any(|r| r["rule_id"] == "BCS014" && r["actual"] == "_orphan"),
        "{dir_res:#?}"
    );
}

#[test]
fn design_mechanics_detect_data_clump_tramp_data_adjacent_primitives_repeated_dispatch_and_thin_wrapper()
 {
    let clump = "def a(user_token: str, calendar_id: str, tenant_id: str) -> str:\n    return user_token + calendar_id + tenant_id\n\n\ndef b(user_token: str, calendar_id: str, tenant_id: str) -> str:\n    return user_token + calendar_id + tenant_id\n\n\ndef c(user_token: str, calendar_id: str, tenant_id: str) -> str:\n    return user_token + calendar_id + tenant_id\n";
    let clump_res = scan("py", clump);
    let clump_hit = clump_res
        .iter()
        .find(|r| {
            r["rule_id"] == "BCS004"
                && r["message"]
                    == "parameters repeatedly travel together across functions (data clump)"
        })
        .expect("must detect 3-param data clump");
    assert!(
        clump_hit["prescription"]
            .as_str()
            .unwrap()
            .contains("frozen dataclass/struct")
    );

    let tramp = "def build_http(sched, bus, ledger, auth, attachments, status_provider):\n    return make_deps(sched=sched, bus=bus, ledger=ledger, auth=auth, attachments=attachments, status_provider=status_provider)\n";
    let tramp_res = scan("py", tramp);
    let tramp_hit = tramp_res
        .iter()
        .find(|r| {
            r["rule_id"] == "BCS004"
                && r["message"]
                    == "function forwards most parameters untouched to a single callee (tramp data)"
        })
        .expect("must detect tramp data");
    assert!(
        tramp_hit["prescription"]
            .as_str()
            .unwrap()
            .contains("make_deps")
    );

    let adjacent = "def sse_stream(bus: object, thread_key: str, after_seq: int, max_events: int) -> int:\n    return after_seq + max_events\n";
    let adj_res = scan("py", adjacent);
    let adj_hit = adj_res
        .iter()
        .find(|r| {
            r["rule_id"] == "BCS004"
                && r["message"]
                    == "adjacent positional parameters share the same primitive type (easy to swap at call sites)"
        })
        .expect("must detect adjacent primitive positional params");
    assert!(adj_hit["prescription"].as_str().unwrap().contains("`*`"));

    let repeated = "def label(kind: str) -> str:\n    if kind == \"a\":\n        return \"A\"\n    elif kind == \"b\":\n        return \"B\"\n    elif kind == \"c\":\n        return \"C\"\n    elif kind == \"d\":\n        return \"D\"\n    return \"?\"\n\n\ndef icon(kind: str) -> str:\n    if kind == \"a\":\n        return \"1\"\n    elif kind == \"b\":\n        return \"2\"\n    elif kind == \"c\":\n        return \"3\"\n    elif kind == \"d\":\n        return \"4\"\n    return \"0\"\n";
    let rep_res = scan("py", repeated);
    let rep_hit = rep_res
        .iter()
        .find(|r| {
            r["rule_id"] == "BCS016"
                && r["message"]
                    == "repeated string-key if/elif dispatch duplicates routing already defined earlier"
        })
        .expect("must detect repeated dispatch");
    assert!(
        rep_hit["prescription"]
            .as_str()
            .unwrap()
            .contains("handler map")
    );

    let thin = "def _wrap(first: int, second: int) -> int:\n    return compute(first, second)\n\n\ndef run(a: int, b: int) -> int:\n    return _wrap(a, b)\n";
    let thin_res = scan("py", thin);
    let thin_hit = thin_res
        .iter()
        .find(|r| {
            r["rule_id"] == "BCS003"
                && r["message"]
                    == "single-caller thin wrapper forwards identical arguments without hiding complexity"
        })
        .expect("must detect single-caller thin wrapper");
    assert!(
        thin_hit["prescription"]
            .as_str()
            .unwrap()
            .contains("inline")
    );
}

#[test]
fn layer_contracts_enforce_pure_modules_and_forbidden_cross_layer_imports() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("src/domain")).unwrap();
    fs::create_dir_all(root.join("src/infra")).unwrap();
    fs::write(
        root.join("bits.toml"),
        "[[layers]]\nname = \"domain\"\npaths = [\"src/domain/**\"]\npure = true\nforbid_imports = [\"infra\", \"httpx\"]\n\n[[layers]]\nname = \"infra\"\npaths = [\"src/infra/**\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/infra/db.py"),
        "def query_user(uid: str) -> str:\n    return uid\n",
    )
    .unwrap();
    fs::write(
        root.join("src/domain/order.py"),
        "from ..infra.db import query_user\n\n\ndef load_order(path: str, uid: str) -> str:\n    _ = query_user(uid)\n    return open(path).read()\n",
    )
    .unwrap();

    let output = bits(&[
        "style",
        "check",
        root.to_str().unwrap(),
        "--config",
        root.join("bits.toml").to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report = json(&output);
    let results = report["results"].as_array().unwrap();
    assert!(
        results.iter().any(|r| {
            r["rule_id"] == "BCS017"
                && r["severity"] == "violation"
                && r["message"]
                    .as_str()
                    .unwrap()
                    .contains("forbidden layer 'infra'")
                && r["prescription"].as_str().is_some()
        }),
        "{report:#}"
    );
    assert!(
        results.iter().any(|r| {
            r["rule_id"] == "BCS017"
                && r["severity"] == "violation"
                && r["message"] == "pure function must not call IO-tainted code"
                && r["actual"] == "open"
        }),
        "{report:#}"
    );
}

#[test]
fn rust_dogfood_fixes_eliminate_brace_let_tuple_and_local_static_false_positives()
 {
    let source = "use std::sync::OnceLock;\n\nfn check(flag: bool, pair: (i32, i32)) -> i32 {\n    static CACHE: OnceLock<i32> = OnceLock::new();\n    let (left, right) = pair;\n    if flag\n        && left > 0\n        && right > 0\n    {\n        return *CACHE.get_or_init(|| left + right);\n    }\n    0\n}\n";
    let results = scan("rs", source);
    assert!(
        results.is_empty(),
        "unexpected Rust false positives: {results:#?}"
    );
}
