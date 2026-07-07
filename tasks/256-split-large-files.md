# Task 256: Address strict linting violations

## Status: IN PROGRESS

## Goal

Bring every `*.rs` file in the workspace into compliance with the build-time linter contract:

- Max **500** lines/file
- Max **40** lines/function
- Max **10** cyclomatic complexity
- Applies to **every** `*.rs` file in the workspace
- No file, directory, or `#[allow(...)]` exemptions

## Completed Work

- `build.rs` now enforces the 500/40/10 contract on all `*.rs` files.
- `docs/linter-rules.md` and `EXECUTE.md` reflect the strict contract.

## Remaining Violations

Run `cargo check` for the live list. The latest run reported violations across `crates/quench-runtime/`, `src/`, and `xtask/`.

### File length violations (sample)
| File | Lines | Over by |
|------|-------|---------|
| `crates/quench-runtime/src/interpreter.rs` | 1226 | +726 |
| `crates/quench-runtime/src/lower.rs` | 984 | +484 |
| `crates/quench-runtime/src/value.rs` | 722 | +222 |
| `src/main.rs` | 658 | +158 |
| `src/bridge/ffi.rs` | 615 | +115 |
| `crates/quench-runtime/tests/runtime_issues.rs` | 897 | +397 |

### Function length violations (sample)
| File | Line | Body lines |
|------|------|------------|
| `src/bridge_reg.rs` | 15 | 380 |
| `src/main.rs` | 125 | 426 |
| `crates/quench-runtime/src/interpreter.rs` | 353 | 416 |
| `crates/quench-runtime/src/lower.rs` | 367 | 213 |
| `crates/quench-runtime/src/lower.rs` | 637 | 156 |

### Complexity violations (sample)
| File | Line | Complexity |
|------|------|------------|
| `crates/quench-runtime/src/builtins/array.rs` | 26 | 77 |
| `crates/quench-runtime/src/interpreter.rs` | 353 | 70 |
| `crates/quench-runtime/src/interpreter.rs` | 186 | 28 |
| `crates/quench-runtime/src/value.rs` | 515 | 20 |
| `src/bridge_reg.rs` | 15 | 16 |

## Recommended Approach

1. Split files over 500 lines into focused submodules.
2. Split the longest functions first; extract helpers.
3. Reduce complexity by introducing early returns and smaller match arms.
4. Keep test files under the same limits; move large test suites into module-based test files.

## Verification

```bash
cargo check
```

The build succeeds only when the linter reports zero violations.

## Dependencies

- Prerequisite for Task 85 (trampoline interpreter) to avoid conflicts in already-large files.
- Related to Task 88 (Rust runtime leverage).

## Targets

- **Suite:** `tooling`
- **Batch:** 7
- **Target subset:** n/a (code hygiene)
- **Blocked by:** none
- **Exit criteria:** All source files pass the 500-line / 40-function limits enforced by the pre-commit linter.
