# Task 07: Adapt runtime.js and remove rquickjs from the project

## Goal

Cut the QuickJS dependency out of the build and make `runtime.js` run on the new interpreter.

## Files

- Modify: `src/runtime.js`
- Modify: `Cargo.toml`
- Modify: `build.rs`
- Modify: `src/main.rs`
- Modify or delete: `src/ink_js.rs`

## Steps

1. Run the interpreter over `src/runtime.js`; fix any unsupported syntax:
   - Replace destructuring defaults if unsupported with explicit `options = options || {}`.
   - Replace any `class` usage with constructor functions/prototypes.
   - Replace template literals with string concatenation.
   - Replace optional chaining/spread if used.
   - Keep `Map`/`Set` if Task 04 implemented them; otherwise polyfill or rewrite.
2. Remove from `Cargo.toml`:
   - `rquickjs = { ... }` dependency
   - update `description` if it still mentions rquickjs
3. Update `build.rs`:
   - Remove the QuickJS bytecode placeholder (`generate_bytecode_bundle` can stay but must not reference QuickJS APIs; make it bundle plain JS only).
4. Remove all `use rquickjs::...` imports from `src/main.rs`, `src/event_loop.rs`, `src/ink_js.rs`, `src/bridge_config.rs`.
5. Delete `src/ink_js.rs` if its only purpose was rquickjs registration; otherwise keep the tag constants.
6. Search the repository for remaining `rquickjs` or `quickjs` references and eliminate them.

## Boundaries

- Remove only `rquickjs` references and QuickJS-specific build logic.
- Keep the linter in `build.rs` unchanged; only strip the QuickJS bytecode generation if it references QuickJS APIs.
- Do not use this task as an excuse to refactor `src/bridge/`, `src/ink/`, `src/render/`, or `src/compiler/`.

## Acceptance criteria

- `grep -R "rquickjs\|quickjs" src Cargo.toml build.rs` returns nothing.
- `cargo build` succeeds.
- `runtime.js` loads into the interpreter without parse/runtime errors.

## Verification

```bash
grep -R "rquickjs\|quickjs" src Cargo.toml build.rs || true
cargo build
```
