# Native support for `.ts/.js/.tsx/.jsx` and React optimizations

## Goal

`quench-runtime` must parse and execute `.ts`, `.js`, `.tsx`, and `.jsx` source directly, without any external cross-compilation step. The main `quench` binary must stop using `esbuild`/`npx` and hand source straight to the runtime. JSX must be transformed inside the runtime with React-aware optimizations.

## Non-goal

- Keeping `esbuild` as a required build tool.
- Supporting every TypeScript-only declaration (`interface`, `type alias`, `namespace`, `declare`) at full semantic fidelity — they are stripped, not type-checked.
- Full ES-module loader with path resolution in the first milestone (module parsing is supported; execution of `import`/`export` may be limited initially).

## Current state

- `quench-runtime` parses only ES scripts via `Syntax::Es` + `parse_script()`.
- TypeScript annotations, JSX, and ES modules are rejected or silently dropped.
- The main `quench` binary runs `npx esbuild` to compile `.ts/.tsx/.jsx` to JS before evaluation.

## Target state

```rust
let mut ctx = Context::new()?;
ctx.eval_file("examples/counter.tsx")?;      // native TSX
ctx.eval("const n: number = 1; n;")?;        // native TS
ctx.eval("const el = <Box />;")?;            // native JSX → ink.createElement
```

## Architecture

```
Source file (.ts/.tsx/.js/.jsx)
        │
        ▼
┌─────────────────────────────────────────┐
│ swc_ecma_parser with correct Syntax     │
│  - .ts/.tsx → Typescript { tsx: .. }    │
│  - .js/.jsx → Es { jsx: .. }            │
│  - module vs script detection           │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ swc transforms (all Rust)               │
│  - swc_ecma_transforms_typescript::strip│
│  - swc_ecma_transforms_react (classic)  │
│    factory=ink.createElement            │
│    fragment=ink.Fragment                │
│  - optional: jsx/jsxs automatic runtime │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ quench-runtime lowerer                  │
│  - produces HIR                         │
└─────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────┐
│ interpreter                             │
└─────────────────────────────────────────┘
```

## Parser mode selection

| Extension | swc `Syntax` | parser call | transforms |
|-----------|--------------|-------------|------------|
| `.js` | `Es` | `parse_script` or `parse_module` | none |
| `.jsx` | `Es { jsx: true }` | `parse_script` or `parse_module` | React classic → `ink.createElement` |
| `.ts` | `Typescript` | `parse_script` or `parse_module` | `strip` |
| `.tsx` | `Typescript { tsx: true }` | `parse_script` or `parse_module` | `strip` + React classic |

Module vs script detection:

1. If the path ends with `.mjs`/`.mts`, force module.
2. If the source contains any `import`/`export` statement at top level, use `parse_module`.
3. Otherwise use `parse_script`.

## JSX factory and fragment

- **Factory:** `ink.createElement` (defined in `src/runtime.js`).
- **Fragment:** `ink.Fragment`.
- **Runtime:** classic transform in phase 1; automatic `jsx`/`jsxs` runtime in phase 2.

Example:

```tsx
const el = <Box color="red"><Text>hi</Text></Box>;
```

becomes:

```js
const el = ink.createElement(Box, { color: "red" }, ink.createElement(Text, null, "hi"));
```

## React optimizations

1. **Static children arrays.** The JSX transform detects children that are all literal/primitive/static and emits a pre-allocated array: `ink.createElement(Box, null, [a, b, c])` instead of spread/flatten at runtime.
2. **Key hoisting.** Static `key` props are kept as plain props (not special objects) to avoid runtime key extraction overhead.
3. **`jsx`/`jsxs` automatic runtime (phase 2).** Emit `ink.jsx(type, props, key)` and `ink.jsxs(type, props, key)` for children-known-static vs dynamic cases, with a lightweight `ink/jsx-runtime` native module.
4. **Compile-time constant folding for `ink.Fragment` with single static child.**

## Main binary changes

- Remove `compiler::compile_file()` / `compile_tsx()` / `compile_ts()` calls from `src/main.rs` and `src/cli.rs`.
- Use `ctx.eval_file(path)` directly, selecting the parser mode from the extension.
- Keep `--prop` injection and shims in `src/main.rs`, but stop rewriting hooks/components with string replacement.
- Remove `src/compiler/mod.rs` dependency on `npx esbuild`.

## HIR / module support

- Add `Program::Module(Vec<ModuleItem>)`.
- Add HIR nodes for `ImportDecl`, `ExportNamedDecl`, `ExportDefaultDecl`, `ExportAllDecl`.
- Initially execute modules as scripts (ignore bindings) if module loader is not ready.

## Files affected

- `crates/quench-runtime/Cargo.toml` — add transform crates.
- `crates/quench-runtime/src/swc_parse.rs` — mode selection, transforms.
- `crates/quench-runtime/src/lib.rs` — `eval_file`, `eval_ts`, `eval_tsx`, `eval_jsx`, `eval_module`.
- `crates/quench-runtime/src/lower.rs` — lower `JSXElement`/`JSXFragment` removed because transform handles it; add module nodes.
- `crates/quench-runtime/src/ast.rs` — add module HIR.
- `src/main.rs`, `src/cli.rs` — use runtime directly.
- `src/compiler/mod.rs` — remove esbuild cross-compilation.
- `src/runtime.js` — expose `ink.jsx`/`ink.jsxs` if automatic runtime is used.
- `docs/architecture.md`, `EXECUTE.md`, `tasks/index.json`.

## Acceptance criteria

- `cargo run -- examples/counter.tsx` works without `esbuild`/`npx`.
- `cargo run -- examples/use-bridge.tsx` works.
- `cargo test -p quench-runtime` passes.
- `examples/` are not modified.
- New regression tests cover native TS, JSX, TSX, and module parsing.
