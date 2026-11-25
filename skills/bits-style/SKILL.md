---
name: bits-style
description: Apply Bits’s Coding Style safely in any repository. Use when adopting the style, reviewing or refactoring changed code, creating a baseline, enabling diff-only or CI checks, or using bits:allow/bits:prove without turning the checker into a full-repo mechanical rewrite.
---

# Bits’s Coding Style

Use Bits to find complex logic, not to manufacture more functions and state wrappers.

Canonical references:

- `STYLE.md` — rules and language profiles.
- `PROTOCOL.md` — scan, warning, violation, unsupported and output contracts.
- `docs/rust-support.md` — Rust’s current partial coverage.

## Non-goals

- Do not make Bits the only correctness gate. Keep the project’s compiler, formatter, linter and real tests.
- Do not rewrite the whole repository to reach zero violations.
- Do not treat `unsupported` as passed, or `warning` as a hard failure.
- Do not change public JSON, Pydantic, FFI, ABI, framework callback or other external naming contracts to satisfy a rule.

## Required inputs

Resolve these before acting:

- `bits` executable and its version. The current integration version is `v0.4.1`.
- Repository root and the Git revision used for comparisons, normally `origin/main`.
- Whether this is first adoption, incremental work, or CI hard-gate adoption.
- Existing language formatters and tests. Bits supplements them.

## Workflow

### 1. Establish the baseline safely

If `bits.toml` does not exist, copy `templates/bits.toml` and adjust only project-specific exclusions.

If `bits-baseline.json` does not exist, generate it once:

```bash
bits style report . --output bits-baseline.json
```

The baseline is a snapshot of existing diagnostics, not a blanket exemption. Update it deliberately after reviewing changes. New files never inherit baseline violations.

If the repository provides `AGENTS.md`, append the short integration section from `templates/AGENTS.md`; otherwise copy that file. Do not replace existing project instructions.

### 2. Inspect before changing code

```bash
bits style report . --format json --output bits-report.json
```

Group results in this order:

1. Parse errors and unsupported files.
2. High-confidence violations: dependency cycles, unreachable code, empty catch, real parameter reassignment, nested ternaries, real nesting and cyclomatic complexity.
3. Warnings: function size, parameter count, naming, comments, format and ambiguous semantics.
4. False-positive candidates and external contracts.

Do not treat every result as a refactoring instruction. If a result is uncertain, preserve behavior and record the uncertainty instead of “fixing” it.

### 3. Apply the smallest safe fix

For incremental work, use:

```bash
bits style fix --diff --commit <base-revision>
```

`fix` is restricted to deterministic whitespace and indentation changes. It does not perform structural refactoring and does not touch unchanged lines in diff mode.

For semantic violations, prefer the smallest change that removes the actual problem:

- Delete dead code and dependency cycles instead of adding abstraction.
- Replace nested control flow with a clearer sequence or data structure when that is simpler.
- Preserve resource lifetimes and ownership boundaries when splitting logic.
- Keep related stages together when moving them would add context objects or hidden state.

Warnings do not require comments. Follow this resolution order: **delete redundant layers → replace branches with data/schema/handler binding → split by responsibility → register at most one `bits:prove`**.

Distinguish the two annotations strictly:

- `bits:allow(<rule>): <reason>` — ONLY for external hard contracts (ABI, FFI, framework signatures, external naming, pure data tables). Unused `bits:allow` is a hard error (`BCS015`) and must be deleted.
- `bits:prove(<rule>[, test=<test_name>]): <why this is the minimal form>; <concrete risk if removed/simplified>` — scarce tech-debt registration for warnings only. It never waives hard violations, allows at most **1 new proof per diff**, and must be verified by an **independent Critic or human** (never self-approved by the author agent). High-risk boundary claims must point to a real test. Unused `bits:prove` is a hard error.

For pass-through layers, keep only invariants owned by this module (reserved keys, auth, structural envelope); let downstream validation reject invalid provider parameters and wrap/forward the original error at the boundary. When a framework (e.g. `argparse`) has already parsed a structure, bind handlers directly (e.g. `set_defaults(handler=...)`) instead of re-branching on command strings.

Functions marked `bits:spec(pure)` must stay free of IO taint (`BCS017`): pass data in as parameters and move `open`/network/process calls to an outer adapter, declaring project-specific IO entrypoints via `io_sources` in `bits.toml`. When refactoring an existing complex function in `--diff` mode, reduce its nesting or cyclomatic complexity so any remaining `BCS003` overage is marked `improving = true` by the ratchet; never add complexity to an already-over-threshold function or create new over-threshold functions.

### 4. Verify behavior and coverage

Run the same scan again, then the project’s tests, formatter and type checks. Confirm:

- No new high-confidence violation was introduced.
- No external contract was broken.
- No unsupported file was silently counted as covered.
- `coverage` and `unsupported` still mean what they say.
- The fix is idempotent: a second `bits style fix --diff --commit <base-revision>` produces no new diff.

Report violations and warnings separately. A clean `bits style check` does not prove the whole repository is simple or correct.

## Adoption modes

### Agent-only review

Use `bits style report` as input to focused review. Do not make it a hard gate yet.

### Incremental gate

```bash
bits style check --diff --commit <base-revision>
```

Existing code uses the baseline; new and changed lines must satisfy the high-confidence rules. Keep warnings as review input.

### Full gate

Enable only after the project has verified its fixtures and false-positive rate:

```bash
bits style check .
```

Start with high-confidence violations. Do not require the entire repository to be warning-free.

## L2 Taste Review (Independent Critic)

Use the four elegance dimensions as focused questions. Scope strictly to:

1. Functions in the current diff with substantial edits or added branches.
2. Functions flagged with L1 warnings (`BCS003`, `BCS016`, etc.) or carrying `bits:prove`.

Do not review untouched files. **If no concrete issue is found, stay silent** — output no praise, summaries of good code, or generic advice.

Ask these four questions on each target function:

- **Level purity**: Do a few lines suddenly drop to a lower abstraction level than the surrounding orchestration (e.g. hand-building dicts/imports inside a lifecycle dispatcher)?
- **Control-flow flatness**: Are branches making real business decisions, or simulating a lookup table, re-dispatching what a framework already parsed, or only reformatting the same error text?
- **Information gain**: Which downstream decision consumes each parsed/validated value? If data is validated and immediately passed unchanged to an IO boundary that already validates it, or parsed JSON falls back to raw text everywhere, delete the mirror check or speculative parser.
- **Symmetry**: Is the same setup call pasted at the head of mutually exclusive branches, or are sibling features handled with inconsistent control flow?

When reviewing a `bits:prove` (`obligation = critic_review`), act as an independent judge with no loyalty to the author's rationale:

- Verify the named `test=` exists and actually exercises the claimed failure scenario.
- Ask: "If we deleted this layer or hoisted/collapsed this shape, would any observable behavior or invariant actually break?"
- Reject vague rationale (`single table`, `keep API`, `cohesive logic`) and request refactoring.

## Agent completion contract

A change is complete only when all are true:

- The requested fix is implemented without changing external contracts.
- The same scanner reports no new high-confidence violation.
- Project tests, formatter and type checks pass.
- The report identifies remaining warnings, unsupported files and low-confidence items explicitly.
- No exemption or explanation was added merely to avoid a useful refactor.

If any of these is not true, the task is not complete; report the remaining blocker instead of declaring success.

## Command reference

```bash
bits style report . --format json --output bits-report.json
bits style check --diff --commit origin/main
bits style fix --diff --commit origin/main
bits style check --baseline bits-baseline.json
bits style report src --format sarif
bits bench --offline
```

`--commit` accepts a SHA, branch, tag or `HEAD~n`. `check` returns 1 for violations, 2 for scan/unsupported errors, and 0 for no violations or warnings. `report` returns 0 after producing a report, including reports containing violations or unsupported files.
