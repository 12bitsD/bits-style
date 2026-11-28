# Bits’s Coding Style

Version: v0.6.1.

Canonical documents: `STYLE.md` and `PROTOCOL.md` from https://github.com/12bitsD/bits-style.

## Workflow

1. Run `bits style check --diff --commit origin/main --format compact` (add `--violations-only` when warnings are noisy) or `bits style report . --json` for full triage.
2. Fix high-confidence violations first: dependency cycles, unreachable code, empty or bare-return catch-all (`except Exception: return`), `nonlocal` cross-closure mutation, parameter reassignment, nested ternaries (including parenthesized), 80-column lines (outside strings/templates/URLs), real nesting (>3) / cyclomatic complexity (>10), and `[[layers]]` / `bits:spec(pure)` boundaries.
3. Follow `hotspots` (exact high-weight lines) and `prescription` (target structural fix) on diagnostics instead of guessing or gaming counters.
4. Run the project’s formatter, type checks, and real tests after every fix.
5. Use `bits style fix --diff --commit origin/main` for deterministic whitespace fixes, `bits style report --diff --commit origin/main --format markdown` for PR review summaries, and `bits style baseline --prune bits-baseline.json .` when stale baseline entries accumulate.

## Guardrails

- Do not perform whole-repository mechanical rewrites to reach zero violations.
- Do not change external JSON, Pydantic, FFI, ABI, or framework callback contracts to satisfy a rule.
- Keep line width (<=80), function size (<=48 lines), and local variables (<=10) tight through genuine simplification and clean decomposition — never by cramming unreadable nested ternaries, packing unrelated values into anonymous tuples, or manufacturing single-caller thin wrappers.
- Act on design-mechanics prescriptions: return structured results instead of mutating multiple out-param containers; group repeated data clumps into value objects; eliminate tramp-data forwarding; use keyword-only `*` / typed options for adjacent same-type primitive params; consolidate repeated string `if/elif` dispatch; delete dead private helpers/constants.
- Warnings do not require comments. `bits:allow(rule): reason` is only for external hard contracts; `bits:prove(rule[, test=name]): minimal reason; concrete risk` is for warnings only, max 1 new per diff, requires independent Critic/human approval, and `test=name` must resolve to a unique real test definition. Unused `allow`/`prove` annotations and newly added legacy `bits:explain` in diff mode are hard errors.
- Keep `bits:spec(pure)` functions and `pure = true` `[[layers]]` modules free of direct or transitive IO (`BCS017`), and respect `forbid_imports` / `allow_imports` layer contracts in `bits.toml`.
- In diff mode, never increase nesting or cyclomatic complexity in an already-over-threshold function, and never add new over-threshold functions; reduce them so the `improving` ratchet applies.
- For L2 taste review, inspect only changed/branchy diff functions and L1 warning sites using level purity, control-flow flatness, information gain, and interface tax. Stay silent when there is no concrete issue.
- `unsupported` is skipped in directory/`--diff` scans but never counts as covered (use `--strict-unsupported` to fail on it). Compile errors, formatter errors, and real test failures remain hard failures.
- A task is complete only when the same scanner reports no new high-confidence violation, all project checks pass, and remaining warnings/skipped unsupported items are reported honestly.
