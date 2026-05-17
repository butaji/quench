# Making the Rune Transpiler Agent-Friendly

Design the compiler so that AI coding assistants can **generate valid Rune**, **debug errors autonomously**, and **extend the language** without understanding the entire codebase.

---

## 1. Deterministic, Idempotent Output

**Rule:** Same `.r.ts` input → byte-identical `.rs` output (modulo timestamps).

**Why:** Agents cache results and diff outputs. Non-deterministic output (random variable names, unstable ordering) creates false deltas and confuses the agent into thinking it broke something.

**Implementation:**
```rust
// codegen.rs — deterministic ordering
fn emit_struct_fields(fields: &[Field]) -> TokenStream {
    // Sort by source declaration order, NOT HashMap iteration order
    fields.iter()
        .sorted_by_key(|f| f.span.lo)  // stable sort by byte offset
        .map(|f| quote! { pub #f.name: #f.ty, })
        .collect()
}
```

**Enforce:** CI runs `rune transpile --check-determinism` — transpile twice, assert identical output.

---

## 2. Structured Errors (JSON + Human)

Agents parse errors programmatically. Don't make them regex human text.

```json
{
  "errors": [
    {
      "severity": "error",
      "code": "RUNE0003",
      "message": "borrow of moved value",
      "primary": {
        "file": "src/handlers/keyboard.r.ts",
        "line": 42,
        "column": 15,
        "snippet": "console.log(s)",
        "label": "value borrowed here after move"
      },
      "secondary": [
        {
          "file": "src/handlers/keyboard.r.ts",
          "line": 41,
          "column": 5,
          "snippet": "consume(s)",
          "label": "value moved here"
        }
      ],
      "suggestion": {
        "message": "consider cloning",
        "replacement": "consume(s.clone())",
        "span": { "line": 41, "column": 13, "len": 1 }
      },
      "rust_equivalent": {
        "message": "rustc E0382",
        "explanation": "In Rust, String is moved on function call. Use .clone() to keep the original."
      }
    }
  ]
}
```

**CLI:** `rune check --json` for agents. `rune check` for humans.

**Why this matters:** An agent can read the JSON, apply the `suggestion.replacement` automatically, and verify the fix with `rune check --json` again.

---

## 3. One-Pass-Per-File Architecture

Each compiler phase is a standalone module with a single public function. Agents reason about one phase at a time.

```
rune/
├── src/
│   ├── main.rs              # CLI entry, 40 lines
│   ├── driver.rs            # Orchestrates passes, 80 lines
│   ├── parse.rs             # SWC → Rune AST, 120 lines
│   ├── validate.rs          # Subset rules, 200 lines
│   ├── infer.rs             # Type & ownership inference, 300 lines
│   ├── lower.rs             # Rune AST → Rust HIR, 250 lines
│   ├── codegen.rs           # HIR → TokenStream, 150 lines
│   ├── error.rs             # Error formatting (JSON + human), 100 lines
│   └── map.rs               # Source map management, 80 lines
```

**Max 300 lines per file, max 40 lines per function.** Clippy enforces this. An agent can read any file in one context window.

**Pass contract:**
```rust
// Each pass has identical signature
pub fn parse(input: &str, file: &Path) -> Result<RuneAst, Vec<Error>>;
pub fn validate(ast: &RuneAst) -> Result<(), Vec<Error>>;
pub fn infer(ast: &mut RuneAst, ctx: &InferCtx) -> Result<(), Vec<Error>>;
pub fn lower(ast: &RuneAst, ctx: &LowerCtx) -> Result<RustHIR, Vec<Error>>;
pub fn codegen(hir: &RustHIR) -> TokenStream;
```

An agent debugging a type inference bug only needs to read `infer.rs`. It doesn't touch `codegen.rs`.

---

## 4. Self-Describing Intermediate Representation (HIR)

The Rust HIR is a plain Rust struct with `#[derive(Debug)]`. Agents can dump it and reason about it.

```rust
// lower.rs — HIR is just data
#[derive(Debug, Clone)]
pub enum RustExpr {
    Lit(RustLit),
    Var(String, RustTy),
    Call { func: Box<RustExpr>, args: Vec<RustExpr>, is_mut: bool },
    MethodCall { receiver: Box<RustExpr>, method: String, args: Vec<RustExpr> },
    Match { scrutinee: Box<RustExpr>, arms: Vec<RustArm> },
    Block(Vec<RustStmt>, Option<Box<RustExpr>>),
    Borrow { expr: Box<RustExpr>, mutability: bool },
}

#[derive(Debug, Clone)]
pub struct RustArm {
    pub pat: RustPat,
    pub guard: Option<RustExpr>,
    pub body: RustExpr,
}
```

**Debug flag:** `RUNE_DEBUG_HIR=1 rune transpile foo.r.ts` prints the HIR as pretty-printed Rust structs. An agent can read this to verify its mental model of the lowering.

---

## 5. Example-Driven Rule Engine

Every validation rule is a struct with **before/after examples**. The agent learns the language by reading the rules.

```rust
// validate.rs
pub static RULES: &[Rule] = &[
    Rule {
        id: "no-any",
        severity: Error,
        check: |ty| matches!(ty, TsType::TsKeywordType(kw) if kw.kind == TsAnyKeyword),
        message: "Type 'any' requires dynamic dispatch. Use a concrete type.",
        example_before: "const x: any = 5;",
        example_after: "const x: number = 5;",
        rust_equivalent: "any maps to no Rust type. Use i32, f64, String, etc.",
    },
    Rule {
        id: "no-class",
        severity: Error,
        check: |node| matches!(node, ModuleItem::Stmt(Stmt::Decl(Decl::Class(_)))),
        message: "Classes are forbidden. Use plain objects and functions.",
        example_before: "class Point { x: number; y: number; }",
        example_after: "type Point = { x: number; y: number; };",
        rust_equivalent: "Structs are generated from type aliases, not classes.",
    },
    // ...
];
```

**Agent prompt:** *"Add a new rule forbidding `with` statements. Follow the existing Rule struct pattern in validate.rs. Include example_before and example_after."*

---

## 6. Test as Specification

Every language feature has a **roundtrip test**: `.r.ts` input → transpile → compile with rustc → assert output.

```rust
// tests/integration/
//   primitives/
//     input.r.ts
//     expected.rs
//     main.rs (harness that calls expected.rs)
//     stdout.txt (expected program output)
```

Directory structure mirrors the spec sections:
```
tests/
├── 01_primitives/
│   ├── 01_numbers/
│   │   ├── integer_literals.r.ts
│   │   ├── float_literals.r.ts
│   │   └── inference_contexts.r.ts
│   ├── 02_strings/
│   ├── 03_booleans/
│   └── 04_null_option/
├── 02_collections/
│   ├── 01_vec/
│   ├── 02_hashmap/
│   └── 03_tuples/
├── 03_control_flow/
│   ├── 01_switch_match/
│   ├── 02_for_of/
│   └── 03_while/
├── 04_functions/
│   ├── 01_basic/
│   ├── 02_generics/
│   └── 03_async/
├── 05_jsx/
│   ├── 01_ratatui/
│   ├── 02_dioxus/
│   └── 03_leptos/
└── 06_errors/
    ├── borrow_moved/
    ├── type_mismatch/
    └── exhaustive_switch/
```

**Agent workflow:** To add a feature, an agent copies an existing test directory, modifies input/expected, runs `cargo test`, fixes until green. The test suite IS the spec.

---

## 7. Source Maps at Every Layer

Agents need to trace errors back to the original `.r.ts` line, not generated `.rs` line.

```rust
// map.rs
#[derive(Clone, Debug)]
pub struct SourceMap {
    pub rune_file: PathBuf,
    pub rune_line: u32,
    pub rune_col: u32,
    pub rust_file: PathBuf,  // target/rune-cache/...
    pub rust_line: u32,
    pub rust_col: u32,
}

// Every HIR node carries its source map
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: SourceMap,
}
```

**Error attribution:** When rustc reports an error on `target/rune-cache/app/src/main.rs:47:12`, the driver looks up the source map and reports:

```
error[E0382]: borrow of moved value
  --> src/main.r.ts:12:15
   |
11 | consume(s);
   |         - value moved here
12 | console.log(s);
   |               ^ value borrowed here after move
```

**Agent benefit:** Agent edits `main.r.ts:12`, not generated Rust. No confusion about generated code.

---

## 8. Incremental / Partial Compilation

Agents edit one file at a time. Don't rebuild the world.

```rust
// driver.rs
pub fn compile_incremental(
    changed_file: &Path,
    previous_hir: &HashMap<PathBuf, RustHIR>,
) -> Result<HashMap<PathBuf, RustHIR>, Vec<Error>> {
    // 1. Parse only changed_file
    let ast = parse_file(changed_file)?;

    // 2. Validate only changed_file (plus imported interfaces)
    validate(&ast, &module_graph)?;

    // 3. Infer types (cached for unchanged dependencies)
    let ctx = InferCtx::new(&previous_hir);
    infer(&mut ast, &ctx)?;

    // 4. Lower to HIR
    let hir = lower(&ast, &ctx)?;

    // 5. Merge into previous HIR map
    let mut new_hir = previous_hir.clone();
    new_hir.insert(changed_file.to_path_buf(), hir);

    Ok(new_hir)
}
```

**Cache:** `target/rune-cache/.hir/` stores serialized HIR per file. `rune dev` only re-parses changed files.

---

## 9. Prompt-Compact Spec Format

The entire Rune subset must fit in an agent's context window (~8k tokens). Use a **single-file spec** that the agent can be primed with.

```markdown
# Rune Subset (Agent Reference)

## Types
- `number` → `f64` (default), integer literal → `i32`, bitwise → `i32`
- `string` → `String` (heap), literal → `&str`
- `boolean` → `bool`
- `T | null` → `Option<T>`
- Tagged union `{tag:"A",...} | {tag:"B",...}` → Rust enum

## Ownership (inferred)
- `const` → immutable binding
- `let` → mutable binding
- function param read-only → `&T`, mutated → `&mut T`, consumed → `T`
- `.clone()` to escape move

## Forbidden
- `any`, `class`, `new`, `this` (prototypes), `var`, `==` (loose), `try/catch`, `eval`, `obj[key]`

## JSX
- `<Block>` → builder pattern
- `{expr}` → interpolation
- `style={condition ? "bold" : "default"}` → conditional style

## Native interop
- `import {foo} from "native:bar"` → `crate::native::bar::foo`
```

**Agent system prompt:** *"You are a Rune compiler. Given the spec above, transpile the following TypeScript to Rust HIR, then to TokenStream."*

---

## 10. Self-Healing Error Recovery

When the agent generates invalid Rune, the compiler should **suggest the fix** in machine-readable format, not just say "no."

```json
{
  "error": "RUNE0007",
  "message": "Dynamic property access is forbidden",
  "snippet": "obj[key]",
  "suggestion": {
    "action": "replace",
    "replacement": "obj.get(key)",
    "note": "Use Map<K,V> for dynamic keys. Change type annotation from `{[k:string]: T}` to `Map<string, T>`."
  },
  "auto_fixable": true
}
```

The agent can run `rune fix --json` and apply all `auto_fixable` suggestions automatically.

---

## 11. Agent-Observable State Machine

The compiler driver exposes its internal state so agents can debug hangs or loops.

```rust
// driver.rs — observable phases
#[derive(Debug, Serialize)]
pub enum CompilePhase {
    Parsing { file: String },
    Validating { rule: String },
    Inferring { function: String },
    Lowering { module: String },
    Codegen { file: String },
    Linking { crate: String },
}

// Emit phase transitions as JSON lines to stderr
// RUNE_PHASE={"phase":"Parsing","file":"main.r.ts","elapsed_ms":12}
```

**Agent reads stderr** to know where it got stuck. If `Linking` takes 30s, the agent knows it's a Cargo issue, not a Rune issue.

---

## 12. One-Command Reproducibility

Every agent interaction starts from a known state.

```bash
# Agent workflow is always:
rune clean              # wipe all caches
rune check              # validate all .r.ts files
rune transpile --emit   # generate to target/rune-cache/
cargo test -p app       # run integration tests
```

No hidden state. No `.cargo/registry` surprises. `rune clean` is total.

---

## TL;DR Checklist

| Principle | Implementation | Agent Benefit |
|---|---|---|
| **Deterministic output** | Stable sort, no timestamps in generated code | Trusts diff, no false deltas |
| **JSON errors** | `rune check --json` with suggestions | Parses and auto-fixes errors |
| **Small passes** | `parse.rs`, `validate.rs`, `infer.rs`, `lower.rs`, `codegen.rs` | Debug one file at a time |
| **Self-describing HIR** | `#[derive(Debug)]` on all IR nodes | Dumps and inspects lowering |
| **Example-driven rules** | Every validation rule has before/after | Learns language from code |
| **Test as spec** | `tests/01_primitives/01_numbers/` | Copies patterns to add features |
| **Source maps** | Every HIR node carries `.r.ts` span | Errors point to user code |
| **Incremental compile** | Only re-parse changed files | Fast feedback on single-file edits |
| **Compact spec** | Single-file subset reference | Fits in context window |
| **Auto-fixable errors** | `rune fix --json` | Self-healing code generation |
| **Observable phases** | JSON line logging to stderr | Debugs hangs and loops |
| **Clean reproducibility** | `rune clean` wipes everything | Known starting state |

Design the compiler as if **the primary user is an AI that reads code, runs tests, and applies diffs**. Humans are secondary.
