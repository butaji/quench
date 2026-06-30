# Task 25: Apply researched interpreter/AOT/JIT best practices

## Goal

Capture the findings from the online research and make the project follow the highest-leverage, lowest-custom-code path: use battle-tested crates for parsing, diagnostics, string interning, ordered maps, allocation, and code generation; adopt the JS-engine optimizations that matter most.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: implement the subset that unblocks the targeted examples/conformance tests first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.


## Ranked findings

### 1. Do not write a parser/lexer — use swc (already done)

- Boa, StarlingMonkey, and every modern JS engine rely on an existing parser or a generated one.
- `swc_ecma_parser` already gives us TS/JS/TSX parsing.
- Custom lexers are error-prone and a huge maintenance burden.

### 2. Focus on interpreter-level wins first; AOT/JIT is future work

- A fully optimized AST interpreter is 10–30× slower than a bytecode VM, but adding a bytecode VM or JIT now would delay getting Ink apps working.
- The HIR should be structured so a future AOT backend can consume it directly; do not add a bytecode layer in the current phase.

### 3. Object shapes (hidden classes) + inline caches are non-optional

- V8, SpiderMonkey, Hermes, Boa, and JSC all use hidden classes and ICs.
- Cache `(expected_shape, offset)` at property-access and call sites.
- Keep object property order consistent and avoid dynamic property add/remove in hot code.

### 4. NaN-box primitives

- Pack `f64`, object pointer, string pointer, and tags into a single `u64`.
- Makes `Value: Copy` and 64-bit, eliminating a huge amount of allocation and pointer chasing.

### 5. Intern identifiers and property names

- Use `lasso` or `string-interner`.
- Property maps become integer-keyed; comparison is a single `u32` equality.

### 6. Use ordered maps for object properties

- JS requires deterministic `for...in` order in practice.
- `indexmap` with a fast hasher gives order + speed.

### 7. Arena-allocate short-lived state

- `bumpalo` for frames, temporary eval state, and render trees.
- Consider `mimalloc` or `tikv-jemallocator` as the global allocator.

### 8. Rich diagnostics with existing crates

- Use `thiserror` for error enums, `miette` or `ariadne` for source snippets (Task 23).
- Do not hand-roll a diagnostic formatter unless necessary.

### 9. Use Cranelift, not LLVM, if/when AOT/JIT is added later

- Cranelift (`cranelift-module`, `cranelift-object`, `cranelift-jit`) is smaller, faster to compile, and easier to embed than LLVM/`inkwell`.
- Move to `inkwell`/LLVM only if Cranelift lacks a target or optimization we need.

### 10. Lazy-load large data

- Boa improved startup by lazy-loading ICU/intl data.
- Apply the same principle to locale data, regex tables, etc.

## Files

- `Cargo.toml`
- `crates/quench-runtime/Cargo.toml`
- `crates/quench-runtime/src/value/`
- `crates/quench-runtime/src/interpreter/`
- `crates/quench-runtime/src/lower/`
- `crates/quench-runtime/src/ast.rs`

## Steps

1. Ensure `Cargo.toml` declares the intended crates:
   - `lasso` or `string-interner`
   - `indexmap`
   - `bumpalo`
   - `rustc-hash` or `foldhash`
   - `regress`
   - `num-bigint`
   - `cranelift`, `cranelift-module`, `cranelift-object`, `cranelift-jit` (behind a feature flag)
   - `miette` or `ariadne`
2. Update the HIR (Task 22) and performance roadmap (Task 11) to reference this task.
3. When implementing shapes/ICs and NaN boxing, follow the staged plan in Task 11. Do not add a bytecode VM or Cranelift backend now.
4. Avoid adding any custom parser, lexer, register allocator, or LLVM backend.

## Boundaries

- Only modify `crates/quench-runtime/` configuration and source.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` and `tests/typescript/` are immutable.

## Acceptance criteria

- The project dependencies and task descriptions reflect the crate choices above.
- No custom parser/lexer or LLVM backend is introduced before Cranelift is proven insufficient.

## Verification

```bash
cargo check -p quench-runtime
cargo test -p quench-runtime
```
