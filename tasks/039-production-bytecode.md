# Task 039: Production: Bytecode Precompilation

## Goal
Precompile JS bundle to QuickJS bytecode and embed in binary.

## Acceptance Criteria
- [ ] Build script or CI step compiles `dist/bundle.js` to `.qbc` using `qjsc` or rquickjs compile API.
- [ ] `include_bytes!("../dist/bundle.qbc")` embeds in Rust binary.
- [ ] Runtime loads bytecode directly with `ctx.compile_module()` — zero parse overhead.
- [ ] Binary runs standalone without esbuild or source files.
- [ ] Integration test: build release binary, run, verify TUI appears.

## Dependencies
- Task 010

## SPEC Reference
§7 Production Build
