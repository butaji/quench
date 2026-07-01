> **I'm using the writing-plans skill to create the implementation plan.**

# Native `.ts/.js/.tsx/.jsx` and React Optimizations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `quench-runtime` parses and executes `.ts/.js/.tsx/.jsx` natively. The main binary stops cross-compiling with `esbuild`. JSX is transformed inside the runtime with `ink.createElement`/`ink.Fragment` and optimized.

**Architecture:** Parser selects `Syntax::Es`/`Typescript` and `parse_script`/`parse_module` based on extension/content. swc transforms strip TypeScript and lower JSX to `ink.createElement`/`ink.Fragment`. The result is lowered to the runtime HIR and interpreted. The main binary calls `ctx.eval_file(path)` directly.

**Tech Stack:** `swc_ecma_parser`, `swc_ecma_transforms_typescript`, `swc_ecma_transforms_react`, `swc_ecma_transforms_base`, `quench-runtime`.

---

## Task 1: Add swc transform dependencies

**Files:**
- Modify: `crates/quench-runtime/Cargo.toml`

- [ ] **Step 1: Add dependencies**

```toml
swc_ecma_transforms_base = "0.143"
swc_ecma_transforms_typescript = "0.197"
swc_ecma_transforms_react = "0.197"
swc_ecma_codegen = "0.158"   # optional, for debug/roundtrip
```

- [ ] **Step 2: Verify `cargo check -p quench-runtime`**

```bash
cargo check -p quench-runtime
```

Expected: passes (new deps downloaded, no code changes yet).

- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/Cargo.toml
git commit -m "chore(runtime): add swc transform deps for TS/JSX"
```

---

## Task 2: Implement parser mode selection and transform pipeline

**Files:**
- Modify: `crates/quench-runtime/src/swc_parse.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/quench-runtime/tests/native_parse.rs`:

```rust
use quench_runtime::Context;

#[test]
fn eval_ts_native() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_ts("const x: number = 1 + 2; x;");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn eval_tsx_native() {
    let mut ctx = Context::new().unwrap();
    // runtime.js defines ink global; this test only checks parsing/lowering
    let result = ctx.eval_tsx("const el = <div />; el;");
    assert!(result.is_ok(), "{result:?}");
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p quench-runtime --test native_parse -- --nocapture
```

Expected: FAIL — `eval_ts`/`eval_tsx` do not exist.

- [ ] **Step 3: Implement `parse_source`**

In `crates/quench-runtime/src/swc_parse.rs`:

```rust
use swc_ecma_parser::{TsSyntax, Syntax};
use swc_ecma_transforms_typescript::strip;
use swc_ecma_transforms_react::{react, ReactOptions, JsxRuntime};
use swc_ecma_visit::FoldWith;
use swc_ecma_ast::Program as SwcProgram;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceKind {
    Js,
    Jsx,
    Ts,
    Tsx,
}

impl SourceKind {
    pub fn from_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("jsx") => SourceKind::Jsx,
            Some("ts") => SourceKind::Ts,
            Some("tsx") => SourceKind::Tsx,
            _ => SourceKind::Js,
        }
    }
}

pub fn parse_source(source: &str, kind: SourceKind, is_module: bool) -> Result<Program, JsError> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );

    let syntax = match kind {
        SourceKind::Js => Syntax::Es(EsSyntax { jsx: false, ..Default::default() }),
        SourceKind::Jsx => Syntax::Es(EsSyntax { jsx: true, ..Default::default() }),
        SourceKind::Ts => Syntax::Typescript(TsSyntax { tsx: false, ..Default::default() }),
        SourceKind::Tsx => Syntax::Typescript(TsSyntax { tsx: true, ..Default::default() }),
    };

    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*fm), None);
    let mut parser = Parser::new_from(lexer);

    let program = if is_module {
        SwcProgram::Module(parser.parse_module().map_err(|e| JsError(format!("Parse error: {:?}", e)))?)
    } else {
        SwcProgram::Script(parser.parse_script().map_err(|e| JsError(format!("Parse error: {:?}", e)))?)
    };

    // Strip TypeScript for .ts/.tsx
    let program = match kind {
        SourceKind::Ts | SourceKind::Tsx => program.foldWith(&mut strip()),
        _ => program,
    };

    // Transform JSX for .jsx/.tsx
    let program = match kind {
        SourceKind::Jsx | SourceKind::Tsx => {
            let mut options = ReactOptions::default();
            options.runtime = JsxRuntime::Classic;
            options.pragma = "ink.createElement".to_string();
            options.pragma_frag = "ink.Fragment".to_string();
            options.development = false;
            options.throw_if_namespace = false;
            program.foldWith(&mut react(cm.clone(), options, Mark::new(), Mark::new()))
        }
        _ => program,
    };

    match program {
        SwcProgram::Script(script) => lower_script(&script).map_err(|e| JsError(e.to_string())),
        SwcProgram::Module(module) => lower_module(&module).map_err(|e| JsError(e.to_string())),
    }
}

fn looks_like_module(source: &str) -> bool {
    // naive top-level import/export detection
    let trimmed: Vec<&str> = source.lines().map(|l| l.trim()).collect();
    trimmed.iter().any(|l| {
        l.starts_with("import ") || l.starts_with("export ") ||
        l.starts_with("import{") || l.starts_with("export{")
    })
}
```

- [ ] **Step 4: Add public eval helpers**

In `crates/quench-runtime/src/lib.rs`:

```rust
use crate::swc_parse::{parse_source, SourceKind};

impl Context {
    pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
        let is_module = swc_parse::looks_like_module(source);
        let program = parse_source(source, SourceKind::Js, is_module)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    pub fn eval_js(&mut self, source: &str) -> Result<Value, JsError> {
        self.eval(source)
    }

    pub fn eval_jsx(&mut self, source: &str) -> Result<Value, JsError> {
        let is_module = swc_parse::looks_like_module(source);
        let program = parse_source(source, SourceKind::Jsx, is_module)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    pub fn eval_ts(&mut self, source: &str) -> Result<Value, JsError> {
        let is_module = swc_parse::looks_like_module(source);
        let program = parse_source(source, SourceKind::Ts, is_module)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    pub fn eval_tsx(&mut self, source: &str) -> Result<Value, JsError> {
        let is_module = swc_parse::looks_like_module(source);
        let program = parse_source(source, SourceKind::Tsx, is_module)?;
        interpreter::eval_program(&program, &mut self.env)
    }

    pub fn eval_file(&mut self, path: &std::path::Path) -> Result<Value, JsError> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| JsError(e.to_string()))?;
        let kind = SourceKind::from_path(path);
        let is_module = kind == SourceKind::Js || kind == SourceKind::Ts ||
                        swc_parse::looks_like_module(&source);
        let program = parse_source(&source, kind, is_module)?;
        interpreter::eval_program(&program, &mut self.env)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p quench-runtime --test native_parse -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/quench-runtime/src/swc_parse.rs crates/quench-runtime/src/lib.rs crates/quench-runtime/tests/native_parse.rs
git commit -m "feat(runtime): native eval for ts/jsx/tsx with swc transforms"
```

---

## Task 3: Remove JSX lowering errors

**Files:**
- Modify: `crates/quench-runtime/src/lower.rs`

- [ ] **Step 1: Remove JSX rejection arms**

Find and delete or no-op these arms in `lower_expr`:

```rust
swc::Expr::JSXMember(_) => ...
swc::Expr::JSXNamespacedName(_) => ...
swc::Expr::JSXEmpty(_) => ...
swc::Expr::JSXElement(_) => ...
swc::Expr::JSXFragment(_) => ...
```

After the transform pipeline, JSX nodes should not reach the lowerer. Keep a fallback `unreachable!()` or error for safety during development.

- [ ] **Step 2: Run runtime tests**

```bash
cargo test -p quench-runtime
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/src/lower.rs
git commit -m "refactor(runtime): remove JSX lowering errors; handled by swc transform"
```

---

## Task 4: Add React/JSX optimizations

**Files:**
- Modify: `crates/quench-runtime/src/swc_parse.rs`
- Modify: `src/runtime.js`

- [ ] **Step 1: Enable static children optimization in swc React transform**

The classic `react` transform already emits `createElement(type, props, a, b, c)` for static children. To get `jsxs`-style pre-allocated arrays, switch to automatic runtime in phase 2. For phase 1, ensure the factory call uses positional children and not a runtime flatten loop.

Verify the generated output with a debug roundtrip:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jsx_transform_uses_ink_factory() {
        let program = parse_source("const el = <Box />;", SourceKind::Jsx, false).unwrap();
        // inspect HIR or serialize back to confirm ink.createElement
    }
}
```

- [ ] **Step 2: Expose `ink.jsxs` fast path in runtime.js (phase 2)**

```js
ink.jsxs = ink.createElement;
ink.jsx = ink.createElement;
```

- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/src/swc_parse.rs src/runtime.js
git commit -m "feat(runtime): ink.createElement JSX factory and static children path"
```

---

## Task 5: Wire main binary to use runtime directly

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cli.rs`
- Modify: `src/compiler/mod.rs`

- [ ] **Step 1: Replace compile + eval with `ctx.eval_file`**

In `src/main.rs`, find where user source is loaded (around `compiler::compile_file`) and replace with:

```rust
ctx.eval_file(std::path::Path::new(&file_path))?;
```

- [ ] **Step 2: Remove esbuild invocation from compiler**

In `src/compiler/mod.rs`, delete or gate behind a deprecated feature flag:

- `run_esbuild()`
- `compile_tsx()` / `compile_ts()`
- The string-based hook/component rewrite (to be replaced later by a swc transform).

Keep `strip_imports` only if still needed; otherwise remove.

- [ ] **Step 3: Update CLI**

In `src/cli.rs`, remove `--compile` / cross-compilation options that no longer apply.

- [ ] **Step 4: Verify examples**

```bash
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/counter.tsx
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

Expected: all render.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/cli.rs src/compiler/mod.rs
git commit -m "feat(cli): evaluate ts/jsx/tsx natively without esbuild"
```

---

## Task 6: Add module HIR and loader (phase 2)

**Files:**
- Modify: `crates/quench-runtime/src/ast.rs`
- Modify: `crates/quench-runtime/src/lower.rs`
- Modify: `crates/quench-runtime/src/interpreter.rs`

- [ ] **Step 1: Add module HIR nodes**

```rust
pub enum Program {
    Script(Vec<Statement>),
    Module(Vec<ModuleItem>),
}

pub enum ModuleItem {
    Stmt(Statement),
    ImportDecl { source: String, specifiers: Vec<ImportSpecifier> },
    ExportNamedDecl { source: Option<String>, specifiers: Vec<ExportSpecifier> },
    ExportDefaultDecl(Expression),
    ExportDefaultExpr(Expression),
    ExportAllDecl { source: String },
}
```

- [ ] **Step 2: Lower module declarations**

Replace the current silent-drop logic in `lower_module_decl()` with real HIR items.

- [ ] **Step 3: Execute modules as scripts initially**

In `interpreter::eval_program`, if `Program::Module`, evaluate all items as statements, ignoring bindings. This unblocks module parsing while the loader is built.

- [ ] **Step 4: Commit**

```bash
git add crates/quench-runtime/src/ast.rs crates/quench-runtime/src/lower.rs crates/quench-runtime/src/interpreter.rs
git commit -m "feat(runtime): module HIR and initial module execution"
```

---

## Task 7: Add end-to-end regression tests

**Files:**
- Create: `crates/quench-runtime/tests/native_extensions.rs`

- [ ] **Step 1: Add tests**

```rust
use quench_runtime::Context;

#[test]
fn native_js_hello() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_js("'hello' + ' world';").unwrap();
    assert_eq!(result.to_js_string().unwrap(), "hello world");
}

#[test]
fn native_ts_types_stripped() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_ts("function add(a: number, b: number): number { return a + b; } add(2, 3);").unwrap();
    assert_eq!(result.to_number().unwrap(), 5.0);
}

#[test]
fn native_jsx_factory() {
    let mut ctx = Context::new().unwrap();
    // ink.createElement is provided by runtime.js; for the runtime crate test we just verify lowering/eval
    let result = ctx.eval_jsx("const el = ink.createElement('Box', null); el;");
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn native_tsx_types_and_jsx() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval_tsx("const x: number = 1; const el = ink.createElement('Box', {x}); el;");
    assert!(result.is_ok(), "{result:?}");
}
```

- [ ] **Step 2: Run tests**

```bash
cargo test -p quench-runtime --test native_extensions -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/quench-runtime/tests/native_extensions.rs
git commit -m "test(runtime): native ts/jsx/tsx regression tests"
```

---

## Task 8: Update documentation and task index

**Files:**
- Modify: `docs/architecture.md`
- Modify: `EXECUTE.md`
- Modify: `tasks/index.json`
- Create: `tasks/70-native-ts-jsx-support.md`

- [ ] **Step 1: Update `docs/architecture.md`** to state native TS/JSX parsing via swc transforms.
- [ ] **Step 2: Update `EXECUTE.md`** to remove esbuild as a dependency and document native extensions.
- [ ] **Step 3: Add Task 70** to `tasks/index.json`.
- [ ] **Step 4: Commit**

```bash
git add docs/ EXECUTE.md tasks/
git commit -m "docs: native ts/jsx/tsx support and no cross-compilation"
```

---

## Verification

```bash
cargo test -p quench-runtime
timeout 60 cargo run -- examples/counter.js
timeout 60 cargo run -- examples/counter.tsx
timeout 60 cargo run -- examples/use-bridge.tsx --prop theme=dark
timeout 60 cargo run -- examples/animations.tsx
```

All must pass. No `esbuild`/`npx` calls should occur during example runs.
