# Task 25: Enforce mandatory crate choices and reduce custom code

## Goal

The project has chosen a fixed set of battle-tested crates for every non-unique subsystem. This task enforces those choices: add the crates to `Cargo.toml`, replace any custom implementations with them, and prohibit new custom code that duplicates their functionality.

## Pareto & reuse note

- **Prefer existing crates, the Rust standard library, and OS features over custom code.** This rule is non-negotiable for the subsystems listed below.
- Follow the 80/20 rule: replace the highest-leverage subsystems first (JSON, regex, diagnostics, interning, ordered maps).
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Mandatory crate choices

| Subsystem | Mandatory crate(s) | Rationale |
|-----------|--------------------|-----------|
| Parser / TypeScript stripping / JSX transform | `swc_ecma_parser`, `swc_ecma_transforms_*` | Already integrated; use swc transforms instead of hand-rolling TypeScript/JSX lowering. |
| JSON parse/stringify | `serde_json` | Correct, fast, well-tested; delete custom JSON code. |
| ECMAScript regex | `regress` | Pure-Rust ECMAScript-compatible engine. |
| Rich diagnostics | `miette` or `ariadne` | Source spans, labels, snippets; no hand-rolled formatter. |
| String interning | `lasso` | `Rodeo`/`ThreadedRodeo`; property maps become integer-keyed. |
| Ordered maps/sets | `indexmap` / `indexmap::IndexSet` | Deterministic `for...in` order and insertion-ordered Map/Set. |
| BigInt / decimal | `num-bigint` / `rust_decimal` | Standard arbitrary-precision arithmetic. |
| Arena allocation | `bumpalo` | Short-lived frames, temporaries, HIR nodes. |
| Fast integer-keyed maps | `rustc-hash` or `foldhash` | Faster than default hasher for interned keys. |
| Error enums | `thiserror` | Structured, `std::error::Error`-compatible errors. |
| Future AOT/JIT (only when reached) | `cranelift-*` | Smaller/faster to embed than LLVM; do not start with `inkwell`. |

## What to remove / replace

1. **Custom JSON code** in `builtins/json.rs` → `serde_json`.
2. **Any regex implementation** → `regress`.
3. **String-keyed property maps** → `lasso` atoms + `indexmap`.
4. **Custom insertion-order tracking in Map/Set** → `IndexMap`/`IndexSet`.
5. **Hand-rolled diagnostic formatting** → `miette`/`ariadne`.
6. **Manual error structs** → `thiserror` enums.
7. **Any custom BigInt/decimal code** → `num-bigint` / `rust_decimal`.
8. **Frequent small allocations** → `bumpalo` arenas.

## Steps

1. Add to `crates/quench-runtime/Cargo.toml`:
   ```toml
   lasso = "0.7"
   indexmap = "2"
   bumpalo = "3"
   rustc-hash = "2"
   regress = "0.10"
   num-bigint = "0.4"
   rust_decimal = "1"
   thiserror = "2"
   miette = "7"
   ariadne = "0.5"
   serde_json = "1"
   ```
2. Replace custom JSON in `builtins/json.rs` with `serde_json` round-tripping.
3. Replace any regex built-in with `regress`.
4. Introduce `lasso::Rodeo` in the `Context` and intern every identifier/property name at parse time.
5. Switch object property storage to `IndexMap<Atom, Property>`.
6. Switch Map/Set internal storage to `IndexMap`/`IndexSet`.
7. Convert `JsError` and `LowerError` to `thiserror` enums with `miette` diagnostic support.
8. Add `bumpalo::Bump` to `Context` and arena-allocate HIR nodes, frames, and temporary objects.
9. Update Task 11 (performance) and Task 66 (sixth review) to reflect that these choices are now enforced.
10. Add a `CONTRIBUTING.md` note: any PR that introduces custom code for these subsystems must first justify why the mandatory crate cannot be used.

## Boundaries

- Only modify `crates/quench-runtime/` configuration and source.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` and `tests/typescript/` are immutable.

## Acceptance criteria

- All crates above are declared in `Cargo.toml`.
- Custom JSON/regex/diagnostic/interning/map-ordering code is removed or wrapped around the mandatory crate.
- No new custom parser/lexer/regex/JSON/BigInt/diagnostic code is introduced.

## Timeout note

- All test commands must run with a timeout to avoid hangs from interpreter bugs or infinite loops.
- Use the `scripts/run_tests.sh` wrapper (if available) or prefix commands with `timeout 120` / `gtimeout 120`.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
