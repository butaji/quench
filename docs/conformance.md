> Running the test262 and TypeScript conformance harnesses.

# Conformance

External submodules:

```bash
git submodule update --init tests/test262 tests/typescript
```

Run:

```bash
cargo test -p quench-runtime --test test262 -- --ignored --nocapture
cargo test -p quench-runtime --test conformance -- --test-threads=1
```

## Full spec size vs. current subset

| Suite | Total files/cases in spec | Current subset | % of spec exercised |
|---|---|---|---|
| test262 | ~53,683 `.js` files | 431 | **0.8%** |
| TypeScript conformance | ~18,876 `.ts` cases | 376 | **2.0%** |

The subsets are intentionally small so the harness runs in seconds while the runtime is still incomplete. Running the full suites now would produce tens of thousands of failures and hide the signal. The subset grows as features land.

## Latest results on the current subset

| Suite | Subset total | Passed | Failed | Skipped | Pass rate (of subset) | Pass rate (of non-skipped) |
|---|---|---|---|---|---|---|
| TypeScript expressions | 376 | 153 | 223 | 0 | **40.7%** | **40.7%** |
| test262 | 431 | 47 | 86 | 298 | **10.9%** | **35.3%** |

## True spec coverage

| Suite | Passed / total spec files | True coverage |
|---|---|---|
| test262 | 47 / 53,683 | **0.09%** |
| TypeScript | 153 / 18,876 | **0.81%** |

The TypeScript harness runs baseline JS extracted from compiler output, not source TS. The test262 harness now loads real harness includes but still skips tests that require unsupported features or includes that are not yet implemented.

Do not edit `tests/test262/`, `tests/typescript/`, or `examples/`.
