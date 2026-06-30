# quench-runtime Architecture Analysis

**Date**: 2026-06-30  
**Status**: Analysis Complete

## Executive Summary

The quench-runtime crate implements a **conventional JavaScript AST interpreter**, not a functional + reactive HIR as documented. The current implementation is a straightforward recursive-descent evaluator that executes a JavaScript AST produced by swc. There is a significant gap between the documented architecture (HIR with reactive primitives) and the actual implementation.

---

## 1. Current Node Types in `ast.rs`

### Program
- `Program::Script(Vec<Statement>)`

### Statements (`Statement` enum, 16 variants)
| Variant | Purpose |
|---------|---------|
| `VarDeclaration` | var/let/const declarations |
| `FunctionDeclaration` | named function declarations (with async flag) |
| `If` | if/else conditionals |
| `While` | while loops |
| `For` | C-style for loops |
| `ForOf` | for...of iteration |
| `ForIn` | for...in iteration |
| `Block` | block statements |
| `Return` | return statements |
| `Expression` | expression statements |
| `Empty` | empty statements |
| `Break` | break statements (with optional label) |
| `Continue` | continue statements (with optional label) |
| `TryCatch` | try/catch/finally |
| `Throw` | throw statements |
| `ModuleExport` | TypeScript `export = expr` |
| `ClassDeclaration` | class declarations |

### Expressions (`Expression` enum, 31 variants)
| Variant | Purpose |
|---------|---------|
| `Number`, `String`, `Boolean` | Literals |
| `Null`, `Undefined` | Special values |
| `Super` | super keyword |
| `Identifier` | variable references |
| `Unresolved` | typeof on undeclared identifiers |
| `Object` | object literals with getters/setters/spread |
| `Array` | array literals with spread |
| `FunctionExpression` | function expressions |
| `ArrowFunction` | arrow functions |
| `Binary` | binary operations |
| `Unary` | unary operations |
| `Assignment` | simple assignment |
| `CompoundAssignment` | +=, -=, etc. |
| `Call` | function calls |
| `Member` | property access (computed/non-computed) |
| `Conditional` | ternary operator |
| `Update` | ++/-- |
| `New` | new expression |
| `Sequence` | comma operator |
| `BlockExpr` | block expressions |
| `ArrayPattern` | array destructuring |
| `ObjectPattern` | object destructuring |
| `Spread` | spread in calls |
| `ClassExpression` | class expressions |
| `Await` | await expressions |

### Supporting Types
- `BinaryOp` (18 operators)
- `UnaryOp` (5 operators)
- `CompoundOp` (11 operators)
- `UpdateOp` (2 operators)
- `PropertyKey`, `ArrayElement`, `BindingElement`, `ObjectProperty`, `ArrowBody`, `ForInit`, `ForBinding`, `VarKind`, `ClassMember`, `ClassBody`, `SpreadElement`

---

## 2. What Matches the "Functional + Reactive HIR" Design

### ✅ Functional Aspects (Partially Matched)

1. **Expression-oriented AST**: Most nodes produce values; statements are clearly separated from expressions.

2. **Explicit effects**: Mutations are represented as `Assignment`, `CompoundAssignment`, `Call` (for setters), and `Throw`. I/O and rendering happen via host function calls.

3. **First-class functions and closures**: `FunctionExpression` and `ArrowFunction` carry captured variables via the `Environment` closure.

4. **ANF-like flattening (partial)**: The lowering phase does some flattening:
   - Destructuring patterns expand into `VarDeclaration` + `Assignment` sequences
   - Optional chaining desugars into conditionals

### ✅ Conventional AST Aspects (Not HIR)

1. **Nested expressions**: `Binary { op, left: Box<Expression>, right: Box<Expression> }` is nested, not ANF-flattened. For example:
   ```javascript
   a + b * c  // AST: Binary(Add, a, Binary(Mul, b, c))
   // HIR would be: Let(t1 = b * c); Let(t2 = a + t1); t2
   ```

2. **No explicit binding nodes**: Variables are bound implicitly via `VarDeclaration` statements, not via `Let/Binding` HIR nodes with explicit SSA-style assignment.

3. **No effect nodes**: The documentation mentions "explicit effect nodes" for mutations, I/O, and rendering, but these are implicit in `Assignment` and `Call` variants.

4. **Conventional control flow**: `If`, `While`, `For`, etc. are conventional AST nodes, not the ANF-style sequencing with explicit `Jump`/`Branch` nodes.

---

## 3. What Parts Are a Conventional AST Interpreter (NOT HIR)

### ❌ Reactive Primitives (Completely Missing)

The documented reactive primitives **do not exist**:

| Documented Node | Status |
|-----------------|--------|
| `Signal { id, initial }` | NOT IMPLEMENTED |
| `SignalGet { signal }` | NOT IMPLEMENTED |
| `SignalSet { signal, value }` | NOT IMPLEMENTED |
| `Memo { id, deps, compute }` | NOT IMPLEMENTED |
| `Effect { id, deps, callback }` | NOT IMPLEMENTED |
| `Render { id, component, props }` | NOT IMPLEMENTED |

The runtime uses `ink.useState`, `ink.useEffect`, `ink.useMemo` from `runtime.js`, which are implemented in JavaScript, not as first-class HIR nodes.

### ❌ ANF Transformation (Not Applied)

The AST is NOT in ANF (A-normal form):
- Nested expressions are not flattened into `Let` bindings
- No `Temp` nodes for intermediate values
- Complex expressions like `(a + b) * (c + d)` remain deeply nested

### ❌ Serde Serialization (Not Present)

No `#[derive(serde::Serialize, serde::Deserialize)]` on AST nodes. The HIR cannot be cached on disk for AOT.

---

## 4. swc-Specific Leaks in the Public API

### High Severity Leaks

1. **`crate::lower::helpers::atom_to_string`**: Exposes `swc_common::atoms::Atom` type in function signatures.
   ```rust
   pub fn atom_to_string(atom: &Atom) -> String  // Atom is swc-specific
   ```

2. **`crate::lower::helpers::lower_bin_op`**: Accepts `&swc::BinaryOp`.
   ```rust
   pub fn lower_bin_op(op: &swc::BinaryOp) -> Result<BinaryOp, LowerError>
   ```

3. **`crate::lower::helpers::lower_unary_op`**: Accepts `&swc::UnaryOp`.

4. **`crate::lower::helpers::lower_param`**: Accepts `&swc::ParamOrTsParamProp`.

5. **`crate::lower::helpers::extract_prop_key`**: Accepts `&swc::PropName`.

6. **`crate::lower::stmt::lower_module`**: Accepts `&swc::Module`.
   ```rust
   pub fn lower_module(module: &swc::Module) -> Result<Program, LowerError>
   ```

7. **`crate::lower::stmt::lower_script`**: Accepts `&swc::Script`.

8. **`crate::lower::expr::lower_expr`**: Accepts `&swc::Expr`.
   ```rust
   pub fn lower_expr(expr: &swc::Expr) -> Result<Expression, LowerError>
   ```

9. **`crate::lower::decl_var::lower_decl`**: Accepts `&swc::Decl`.

10. **`crate::lower::patterns::expand_nested_pattern`**: Accepts `&swc::Pat`.

### Medium Severity Leaks

11. **`crate::swc_parse` module**: Entire module re-exports swc parser types.

12. **`crate::lower::LowerError`**: Only has `message: String`; no source spans.

---

## 5. Concrete Next Steps for HIR Stabilization

### Priority 1: Add Missing Reactive Primitives

**Files to modify**: `crates/quench-runtime/src/ast.rs`

Add these new variants to `Statement` or create a new `HirNode` enum:

```rust
// Reactive Signal - mutable reactive cell
Signal { id: String, initial: Expression }

// SignalGet - read a signal
SignalGet { signal_id: String }

// SignalSet - write a signal
SignalSet { signal_id: String, value: Expression }

// Memo - cached derived value
Memo { id: String, deps: Vec<Expression>, compute: Vec<Statement> }

// Effect - scheduled side effect
Effect { id: String, deps: Vec<Expression>, callback: Vec<Statement> }

// Render - reactive component boundary
Render { id: String, component: Box<Expression>, props: Vec<(String, Expression)> }
```

### Priority 2: ANF-Like Flattening

**Files to modify**: `crates/quench-runtime/src/lower/expr.rs`

Add `Let` binding nodes:

```rust
// Let binding for ANF transformation
Let { name: String, value: Expression, body: Box<Statement> }

// Temp for intermediate values
Temp { id: usize, value: Expression }
```

Transform nested expressions in lowering:
```javascript
// Input: (a + b) * (c + d)
// Current AST: Binary(Mul, Binary(Add, a, b), Binary(Add, c, d))
// HIR: Let(t1, Add(a, b)); Let(t2, Add(c, d)); Let(t3, Mul(t1, t2)); t3
```

### Priority 3: Serde Serialization

**Files to modify**: `Cargo.toml`, `crates/quench-runtime/src/ast.rs`

Add feature flag and derives:
```toml
[features]
default = []
serde = ["dep:serde"]
```

```rust
#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Statement { ... }
```

### Priority 4: Hide swc-Specific APIs

**Files to modify**: `crates/quench-runtime/src/lower/mod.rs`, `crates/quench-runtime/src/lower/helpers.rs`

Make lowering functions `pub(crate)` and add internal-only markers:

```rust
// In lib.rs
pub mod lower;  // Keep public for now, but add documentation

// In lower/mod.rs
pub(crate) mod helpers;  // Internal - not exposed
pub(crate) mod decl;
pub(crate) mod stmt;
pub(crate) mod expr;
pub(crate) mod patterns;
```

Create a clean public API:
```rust
/// Parse and lower source to HIR
pub fn lower_source(source: &str) -> Result<Program, JsError>;

/// Parse TypeScript and lower to HIR
pub fn lower_typescript(source: &str) -> Result<Program, JsError>;
```

### Priority 5: Add Source Spans to Errors

**Files to modify**: `crates/quench-runtime/src/ast.rs`, `crates/quench-runtime/src/lower/mod.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowerError {
    pub message: String,
    pub span: Option<Span>,
}
```

---

## 6. Summary of Gaps

| Category | Status | Severity |
|----------|--------|----------|
| Reactive primitives (Signal, Memo, Effect, Render) | **NOT IMPLEMENTED** | Critical |
| ANF transformation | **NOT APPLIED** | High |
| Serde serialization | **NOT PRESENT** | High |
| Source spans in errors | **PARTIAL** (Span exists but no line/col) | Medium |
| swc API leaks | **SEVERE** (10+ functions) | High |
| Effect nodes | **IMPLICIT** (in Assignment/Call) | Low |
| Closure representation | **WORKING** (via Environment) | N/A |

---

## 7. Recommendation

**For the current phase**: The conventional AST interpreter is working correctly for all examples and tests. The reactive primitives and ANF transformation are future work that would require:

1. A breaking change to the `ast.rs` types
2. Modifications to the lowering phase
3. Updates to the interpreter to handle new node types

**Recommended approach**:
1. **Do not** add reactive primitives now - they would require a complete rewrite of `runtime.js` and all examples
2. **Add serde serialization** behind a feature flag for AOT caching (non-breaking)
3. **Hide swc-specific APIs** behind `pub(crate)` where possible
4. **Document** the current architecture accurately in `README.md`
5. **Create a separate HIR module** for future reactive work rather than modifying the existing AST

---

## Appendix: File Statistics

| File | Lines | Purpose |
|------|-------|---------|
| `ast.rs` | 258 | AST node definitions |
| `lower/expr.rs` | 942 | Expression lowering |
| `lower/stmt.rs` | 384 | Statement lowering |
| `interpreter/mod.rs` | 222 | Interpreter entry point |
| `interpreter/eval_expr/` | ~1300 | Expression evaluation |
| `interpreter/eval_stmt/` | ~500 | Statement evaluation |
| `value/mod.rs` | 298 | Value representation |
| Total | ~11,358 | Entire crate |
