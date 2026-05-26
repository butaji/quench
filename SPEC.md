# runts Specification

## Specification for Fresh/Preact to Native Rust Compiler

**Version:** 0.3.0  
**Status:** Implementation In Progress  
**Last Updated:** 2026-05-26  
**Binary Size:** ~2.5MB (stripped release)  
**Tests:** 47+ passing  

---

## Table of Contents

1. [Overview](#1-overview)
2. [Supported TypeScript/TSX Subset](#2-supported-typescripttsx-subset)
3. [Architecture](#3-architecture)
4. [Transpilation Pipeline](#4-transpilation-pipeline)
5. [Runtime System](#5-runtime-system)
6. [Islands Architecture](#6-islands-architecture)
7. [Development Mode](#7-development-mode)
8. [Production Build](#8-production-build)
9. [Roadmap](#9-roadmap)
10. [Performance Targets](#10-performance-targets)
11. [Trade-offs](#11-trade-offs)
12. [Migration Guide](#12-migration-guide)

---

## 1. Overview

### 1.1 Mission

**runts** is a compiler that transforms Fresh/Preact TypeScript/TSX code into native Rust binaries, enabling:
- Zero external JS runtime dependencies
- Framework-level Fresh compatibility (95%+ API coverage)
- Fine-grained reactivity via Rust signals
- Partial hydration with islands architecture
- Single-binary deployment with <5KB client runtime

### 1.2 Design Principles

1. **Fresh Compatibility First** — Target 95%+ API compatibility with Fresh's patterns
2. **Minimal Subset** — Ruthlessly exclude rarely-used features; every syntax element must justify its inclusion
3. **Pure Rust Runtime** — No V8, Deno, or WebAssembly JS engines
4. **Native Performance** — Leverage Rust's zero-cost abstractions
5. **Zero-Config Dev Mode** — Instant hot reload without compilation

### 1.3 Key Differentiators

| Feature | runts | Deno Fresh | Next.js |
|---------|-------|------------|---------|
| Runtime | Native Rust | Deno | Node.js |
| Binary size | ~2.5MB | N/A (Deno required) | N/A |
| Client JS | ~5KB | ~15KB (Preact) | Variable |
| Cold start | ~5ms | ~100ms | ~200ms |
| Islands | Full | Full | Partial (RSC) |

### 1.4 What is NOT Supported

These exclusions are intentional to keep the implementation focused:

```typescript
// ❌ NOT SUPPORTED

// Class components - use function components only
class MyComponent extends Component { }

// Legacy context (use createContext with Provider)
const Context = createContext(defaultValue);

// forwardRef
const Button = forwardRef((props, ref) => { });

// React.memo (use signals instead)
const Memoized = memo(Component);

// React.Suspense (use lazy islands)
<Suspense fallback={<Loading />}>

// Portal
createPortal(<Content>, domNode);

// Generator functions
function* gen() { yield 1; }

// Dynamic require() / eval()
require(path);
eval(code);

// TypeScript decorators
@decorator class Foo { }

// Namespace declarations
namespace MyNamespace { }

// Enum declarations (use const objects or as const)
enum Color { Red, Green }

// Browser-only APIs without polyfills:
// - window.localStorage (without adapter)
// - document.querySelector (without adapter)
// - WebSocket (without adapter)
```

---

## 2. Supported TypeScript/TSX Subset

### 2.1 Core Syntax Support

| Feature | Status | Example |
|---------|--------|---------|
| **Type Annotations** | ✅ Full | `let x: number = 5` |
| **Interfaces** | ✅ Full | `interface Props { name: string }` |
| **Type Aliases** | ✅ Full | `type ID = string \| number` |
| **Generic Types** | ✅ Full | `function foo<T>(x: T): T` |
| **Union Types** | ✅ Full | `string \| null \| undefined` |
| **Intersection Types** | ✅ Partial | `A & B` (limited nesting) |
| **Literal Types** | ✅ Full | `"foo"`, `42`, `true` |
| **Template Literal Types** | ✅ Full | `` `prefix${T}suffix` `` |

### 2.2 Expressions

| Feature | Status | Example |
|---------|--------|---------|
| **JSX/TSX** | ✅ Full | `<div class="x">{count}</div>` |
| **Fragments** | ✅ Full | `<>...</>` or `<Fragment>` |
| **Arrow Functions** | ✅ Full | `(x) => x + 1` |
| **Async/await** | ✅ Full | `await fetch(url)` |
| **Destructuring** | ✅ Full | `const { a, b } = obj` |
| **Spread Operator** | ✅ Full | `{...obj}`, `[...arr]` |
| **Template Literals** | ✅ Full | `` `Hello ${name}` `` |
| **Optional Chaining** | ✅ Full | `obj?.prop?.nested` |
| **Nullish Coalescing** | ✅ Full | `x ?? defaultValue` |
| **Ternary** | ✅ Full | `a ? b : c` |
| **Logical Operators** | ✅ Full | `&&`, `\|\|`, `??` |

### 2.3 Statements

| Feature | Status | Example |
|---------|--------|---------|
| **If/else** | ✅ Full | `if (x) {} else {}` |
| **For loops** | ✅ Full | `for (let i = 0; i < n; i++)` |
| **For..of** | ✅ Full | `for (const x of arr)` |
| **For..in** | ✅ Partial | `for (const k in obj)` |
| **While** | ✅ Full | `while (condition) {}` |
| **Switch** | ✅ Full | `switch (x) { case a: }` |
| **Try/catch** | ✅ Full | `try {} catch (e) {}` |
| **Return** | ✅ Full | `return value` |
| **Throw** | ✅ Full | `throw new Error()` |

### 2.4 Preact Hooks

| Hook | Status | Signature | Notes |
|------|--------|-----------|-------|
| `useState` | ✅ Full | `useState<T>(init: T \| () => T)` | Returns `(T, (T) => void)` |
| `useEffect` | ✅ Full | `useEffect(fn, deps?)` | Cleanup via returned fn |
| `useRef` | ✅ Full | `useRef<T>(init: T)` | Returns `Ref<T>` with `.current` |
| `useMemo` | ✅ Full | `useMemo(fn, deps)` | Memoized computation |
| `useCallback` | ✅ Full | `useCallback(fn, deps)` | Stable function reference |
| `useReducer` | ✅ Full | `useReducer(reducer, init)` | Redux-style state |
| `useContext` | ✅ Full | `useContext(ctx)` | Requires Provider |
| `useLayoutEffect` | ✅ Full | `useLayoutEffect(fn, deps)` | Sync before paint |
| `useDebugValue` | ✅ Ignored | `useDebugValue(val)` | No-op in production |
| `useId` | ✅ Full | `useId()` | Returns stable unique string |

### 2.5 Preact Signals

| API | Status | Notes |
|-----|--------|-------|
| `signal(value)` | ✅ Full | Reactive value container |
| `computed(fn)` | ✅ Full | Derived signal |
| `effect(fn)` | ✅ Full | Side effect |
| `batch(fn)` | ✅ Full | Group updates |
| `useSignal` | ✅ Full | Hook variant |
| `useComputed` | ✅ Full | Hook variant |

### 2.6 Fresh-Specific APIs

| API | Status | Notes |
|-----|--------|-------|
| `PageProps<T>` | ✅ Full | Route props with data |
| `HandlerContext` | ✅ Full | Request context |
| `Handler` object | ✅ Full | GET, POST, etc. |
| `IS_BROWSER` | ✅ Full | Runtime detection |
| `UnknownPageProps` | ✅ Full | 404 routes |
| `_app.tsx` | ✅ Full | App wrapper |
| `_layout.tsx` | ✅ Full | Layout nesting |
| `_middleware.ts` | ✅ Full | Request middleware |
| `Head` | ✅ Full | `<Head>` for meta tags |
| `File Routes` | ✅ Full | `/routes/[slug].tsx` |

### 2.7 Supported JSX Attributes

| HTML Attribute | JSX Name | Converted To |
|----------------|----------|--------------|
| `class` | `className` | `class_name` |
| `for` | `htmlFor` | `for_id` |
| `tabindex` | `tabIndex` | `tab_index` |
| `readonly` | `readOnly` | `read_only` |
| `maxlength` | `maxLength` | `max_length` |
| `data-*` | Any | Passed through |
| `aria-*` | Any | Passed through |
| `on*` | Event handlers | `on_*` in Rust |

### 2.8 Type Mapping to Rust

| TypeScript | Rust | Notes |
|------------|------|-------|
| `string` | `String` | |
| `number` | `f64` | |
| `boolean` | `bool` | |
| `null` | `Option<T>` | |
| `undefined` | `Option<T>` | |
| `any` | `serde_json::Value` | |
| `unknown` | `serde_json::Value` | |
| `void` | `()` | |
| `never` | `!` | Unreachable |
| `object` | `serde_json::Value` | |
| `Array<T>` | `Vec<T>` | |
| `ReadonlyArray<T>` | `&[T]` | |
| `Record<K,V>` | `HashMap<K,V>` | |
| `Map<K,V>` | `HashMap<K,V>` | |
| `Set<T>` | `HashSet<T>` | |
| `Promise<T>` | `JoinHandle<T>` | Async |
| `Function` | `Box<dyn Fn()>` | Limited |
| `Date` | `chrono::DateTime` | With adapter |
| `RegExp` | `regex::Regex` | With adapter |

### 2.9 Import/Export Support

```typescript
// Named imports
import { useState, useEffect } from "preact/hooks";
import { signal } from "@preact/signals";

// Default imports
import React from "preact";
import MyComponent from "./components/MyComponent";

// Type-only imports (stripped during transpilation)
import type { SomeType } from "./types";
import { type SomeType, someValue } from "./module";

// Re-exports
export { useState, useEffect } from "preact/hooks";
export { signal } from "@preact/signals";
export default function MyComponent() {}

// Side-effect imports (kept as comments)
import "some/css/file.css";
```

### 2.10 Exclusions (Explicit List)

The following are NOT supported and will produce compile errors:

| Exclusion | Reason | Workaround |
|-----------|--------|------------|
| Class components | Not in Fresh paradigm | Use function components |
| `enum` declarations | Runtime overhead | Use `as const` or objects |
| Namespace declarations | Complexity | Use modules |
| Decorators | Stage 2 proposal | Use function wrappers |
| `with` statement | Forbidden | Restructure code |
| Labeled statements | Rarely used | N/A |
| `do-while` | Rarely used | Use `while` |
| Generator functions | Complexity | Use `async` instead |
| `typeof` on types | Not applicable | TypeScript type system |
| Dynamic `require()` | Security risk | Use static imports |
| `eval()` | Security risk | N/A |

---

## 3. Architecture

### 3.1 High-Level Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                              runts CLI                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  User TS/TSX Source                                                  │
│  ┌─────────────┐   ┌──────────────┐   ┌────────────────────────┐    │
│  │  routes/    │   │  islands/    │   │  components/            │    │
│  │  - index.tsx│   │  - Counter  │   │  - Header.tsx          │    │
│  │  - [id].tsx │   │  - TodoList │   │                         │    │
│  └──────┬──────┘   └──────┬───────┘   └───────────┬────────────────┘    │
│         │                  │                      │                    │
│         └──────────────────┼──────────────────────┘                    │
│                            ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │                     PARSER (Recursive Descent)                   │  │
│  │  ┌─────────────────────────────────────────────────────────────┐  │  │
│  │  │ ~1700 lines of hand-written Rust                           │  │  │
│  │  │ - Tokenizer + AST builder                                  │  │  │
│  │  │ - Full TS/TSX syntax coverage                              │  │  │
│  │  │ - Precise source location tracking                        │  │  │
│  │  │ - Zero external dependencies                              │  │  │
│  │  └─────────────────────────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                            ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │                     ANALYZER                                     │  │
│  │  ┌─────────────────────────────────────────────────────────────┐  │  │
│  │  │ Semantic analysis pass                                     │  │  │
│  │  │ - Island detection (islands/ directory)                   │  │  │
│  │  │ - Route analysis (routes/ directory)                      │  │  │
│  │  │ - Hook usage tracking                                      │  │  │
│  │  │ - Type inference (limited to codegen hints)                │  │  │
│  │  │ - JSX transformation hints                                │  │  │
│  │  └─────────────────────────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                            ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │                     CODE GENERATOR                              │  │
│  │  ┌─────────────────────────────────────────────────────────────┐  │  │
│  │  │ HIR → Rust source                                          │  │  │
│  │  │ - Component #[component] attribute                         │  │  │
│  │  │ - Hook runtime calls (use_state, etc.)                    │  │  │
│  │  │ - JSX → html! macro transformations                        │  │  │
│  │  │ - Route handlers (Axum integration)                        │  │  │
│  │  └─────────────────────────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│                            │                                          │
│           ┌────────────────┴────────────────┐                        │
│           ▼                                 ▼                        │
│  ┌─────────────────────┐       ┌─────────────────────┐              │
│  │   DEV MODE          │       │   PRODUCTION         │              │
│  │   (In-Memory)       │       │   (Static Binary)    │              │
│  │                     │       │                     │              │
│  │ - Interpret HIR      │       │ - Cargo build       │              │
│  │ - No compilation    │       │ - LTO enabled       │              │
│  │ - Instant reload    │       │ - Single binary     │              │
│  │ - WebSocket HMR    │       │ - ~2.5MB output    │              │
│  └─────────────────────┘       └─────────────────────┘              │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 Directory Structure

```
runts/
├── src/
│   ├── main.rs                 # CLI entry point
│   ├── config.rs               # Config loading
│   ├── lib.rs                  # Core exports
│   │
│   ├── transpile/             # Transpilation pipeline
│   │   ├── mod.rs             # Pipeline orchestration
│   │   ├── parser.rs          # Recursive descent (~1700 lines)
│   │   ├── hir.rs             # High-level IR
│   │   ├── analyzer.rs        # Semantic analysis
│   │   ├── codegen.rs         # Rust code generation
│   │   ├── jsx_transformer.rs # JSX-specific transforms
│   │   ├── routegen.rs        # Route handler generation
│   │   ├── middlewaregen.rs   # Middleware chain generation
│   │   └── tests.rs           # Comprehensive tests
│   │
│   ├── runtime/               # Runtime library
│   │   ├── mod.rs             # Module exports
│   │   ├── vdom.rs            # Virtual DOM types (~390 lines)
│   │   ├── signals.rs        # Fine-grained reactivity (~190 lines)
│   │   ├── hooks.rs          # Preact hooks (~380 lines)
│   │   ├── islands.rs        # Islands architecture (~390 lines)
│   │   ├── component.rs      # Component system (~150 lines)
│   │   ├── html.rs           # HTML helpers
│   │   ├── server.rs         # Server utilities (~210 lines)
│   │   └── prelude.rs        # Public API (~65 lines)
│   │
│   └── commands/             # CLI commands
│       ├── mod.rs
│       ├── init.rs           # Project scaffolding
│       ├── dev.rs            # Development server
│       └── build.rs          # Production build
│
├── crates/
│   ├── runts-lib/            # Runtime library crate
│   ├── runts-client/         # Client hydration (minimal JS)
│   └── runts-macros/         # Proc macros
│
├── examples/
│   └── my-blog/
│       ├── routes/
│       │   ├── index.tsx
│       │   ├── _app.tsx
│       │   ├── _layout.tsx
│       │   ├── _middleware.ts
│       │   └── blog/
│       │       └── [slug].tsx
│       ├── islands/
│       │   ├── Counter.tsx
│       │   └── TodoList.tsx
│       ├── components/
│       │   └── Header.tsx
│       └── runts.config.json
│
└── tests/
    └── integration/
        ├── routes.rs
        ├── islands.rs
        └── hooks.rs
```

### 3.3 Component Transpilation Flow

```
┌────────────────────────────────────────────────────────────────────┐
│ Component Transpilation Example                                    │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  TSX Source                     HIR                     Rust         │
│  ──────────────                ───                     ───────      │
│                                                                     │
│  import { useState }       Import {               use runts_lib::*;│
│  from "preact/hooks";      hooks: ["useState"] }                    │
│                                                                     │
│  function Counter(          Function {             #[component]     │
│    { initial = 0 }         name: "Counter",        pub fn counter(  │
│  : CounterProps) {          params: [               initial: f64,  │
│                            { name: "initial",      ) -> VNode {     │
│    const [count,           type: Some("number"),    let (count,    │
│      setCount] =           default: Some(0)         set_count) =   │
│      useState(initial);    } ],                    use_state(||    │
│                           body: [...]              initial);      │
│    return (                }                       // ...           │
│      <div>{count}</div>    }                                         │
│    );                    }                       html! {             │
│  }                                                 <div>            │
│                                                   {count.to_string()}
│                                                   </div>           │
│                                                 }                  │
│                                                                     │
└────────────────────────────────────────────────────────────────────┘
```

---

## 4. Transpilation Pipeline

### 4.1 Parser (Recursive Descent)

The parser is hand-written in Rust (~1700 lines) for:
- **Zero dependencies** — No external parser crates (unlike swc)
- **Speed** — Fast enough for incremental parsing in dev mode
- **Control** — Full control over error messages and recovery
- **Subset optimization** — Only parses features we actually support

**Supported Token Types:**

```
Keywords:        async, await, function, const, let, var, return, 
                 if, else, for, while, do, switch, case, default,
                 break, continue, throw, try, catch, finally,
                 class, interface, type, enum, extends, implements,
                 new, this, super, import, export, default,
                 from, as, of, in, instanceof, typeof, void, delete

Operators:       + - * / % = == === != !== < > <= >= && || ! & | ^ ~
                 ? : => ... .. << >> >>> += -= *= /= %= &= |= ^= <<= >>= >>>=
                 ?? ?. ?.?[

Punctuation:     ( ) { } [ ] , ; : . @ # `

Literals:        String, Number, Boolean (true/false), Null, RegExp, Template
```

**Parser Grammar (Simplified EBNF):**

```ebnf
Module         = { Import | Export | Statement }

Import         = "import" [TypeOnly] ImportSpec "from" String ";"
ImportSpec     = DefaultImport | NamedImports | "*" "as" Ident
DefaultImport  = Ident
NamedImports   = "{" { Ident ["as" Ident] ","? } "}"
Export         = "export" (ExportSpec | DefaultExport) ";"
DefaultExport  = "default" Expression

Statement      = Block | EmptyStmt | ExprStmt | IfStmt | WhileStmt
                | ForStmt | ForInStmt | ForOfStmt | SwitchStmt
                | TryStmt | ThrowStmt | ReturnStmt | BreakStmt
                | ContinueStmt | FunctionDecl | ClassDecl | VariableDecl
                | TypeDecl | InterfaceDecl

FunctionDecl   = ["async"] "function" Ident TypeParams? Params TypeAnnotation? Block

Params         = "(" [Param {"," Param}] ")"
Param          = Pattern TypeAnnotation? ("=" Expression)?

ExprStmt       = Expression ";"
IfStmt         = "if" "(" Expression ")" Statement ["else" Statement]

Expression     = Assignment
Assignment     = Conditional {( "=" | "+=" | "-=" | ... ) Assignment}
Conditional    = LogicalOr ["?" Expression ":" Expression]
LogicalOr      = LogicalAnd {"||" LogicalAnd}
LogicalAnd     = BitOr {"&&" BitOr}
BitOr          = BitXor {"|" BitXor}
BitXor         = BitAnd {"^" BitAnd}
BitAnd         = Equality {"&" Equality}
Equality       = Comparison {("==" | "!=" | "===" | "!==") Comparison}
Comparison     = Range {(">" | "<" | ">=" | "<=") Range}
Range          = Shift {".." Shift}
Shift          = Add {("<<" | ">>") Add}
Add            = Mul {("+" | "-") Mul}
Mul            = Unary {("*" | "/" | "%") Unary}
Unary          = ("!" | "-" | "+" | "typeof" | "void") Unary | Call
Call           = Member {"(" Arguments? ")"} | "await" Unary
Member         = Primary {("." Ident | "[" Expression "]")*}
Primary        = Ident | Literal | Template | Lambda | JSX | "(" Expression ")"

Lambda         = Params "=>" (Expression | Block)

JSX            = "<" JSXTag JSXAttrs? ("/>" | ">" JSXChildren "</" JSXTag ">")
JSXTag         = Ident (":" Ident)? | JSXMember | JSXNamespaced
JSXAttrs       = { JSXAttr }
JSXAttr        = JSXAttrName ("=" JSXAttrValue)?
                | "{" SpreadElement "}"
JSXChildren    = { JSXChild }
JSXChild       = JSX | "{" Expression "}" | StringLiteral

TypeAnnotation = ":" Type
Type           = UnionType | IntersectionType | PrimaryType
UnionType      = PrimaryType {"|" PrimaryType}
PrimaryType    = Ident TypeParams? | "string" | "number" | "boolean"
                | "void" | "null" | "undefined" | "any" | "unknown"
                | "object" | "Array" "<" Type ">"
                | "{" TypeMembers "}" | "(" Type ")" | "[" Type "]"
```

### 4.2 High-Level IR (HIR)

After parsing, the AST is normalized to HIR:

```rust
// From src/transpile/hir.rs
pub enum Expr {
    Ident { name: String },
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    Template { parts: Vec<TemplatePart>, exprs: Vec<Box<Expr>> },
    
    // Binary & Unary
    Bin { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, arg: Box<Expr>, prefix: bool },
    Update { op: UpdateOp, arg: Box<Expr>, prefix: bool },
    Logical { op: LogicalOp, left: Box<Expr>, right: Box<Expr> },
    
    // Functions & Calls
    Call { callee: Box<Expr>, args: Vec<Expr>, type_args: Vec<Type> },
    New { callee: Box<Expr>, args: Vec<Expr> },
    Arrow { params: Vec<Param>, body: Box<Stmt> },
    Function { decl: Box<FunctionDecl> },
    
    // Objects & Arrays
    Object { props: Vec<ObjectProp> },
    Array { elems: Vec<Option<Expr>> },
    
    // JSX
    JSX(JSXExpr),
    
    // Other
    Cond { test: Box<Expr>, consequent: Box<Expr>, alternate: Box<Expr> },
    Assign { left: Box<Expr>, op: AssignOp, right: Box<Expr> },
    Member { object: Box<Expr>, property: Box<Expr>, computed: bool },
    Await { arg: Box<Expr> },
    // ... more variants
}

pub struct FunctionDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Option<Block>,
    pub is_async: bool,
}
```

### 4.3 Semantic Analysis

The analyzer performs these passes:

```rust
// From src/transpile/analyzer.rs
pub struct Analyzer {
    errors: Vec<AnalyzerError>,
    warnings: Vec<AnalyzerWarning>,
    
    // Analysis state
    current_scope: Scope,
    hook_usages: Vec<HookUsage>,
    island_candidates: HashSet<String>,
    route_handlers: Vec<RouteInfo>,
}

impl Analyzer {
    // 1. Island Detection
    // Files in islands/ directory → island components
    // Components with "use client" directive → island components
    fn detect_islands(&mut self, module: &Module, path: &Path) {
        if path.to_string().contains("islands/") {
            self.island_candidates.insert(module.default_export_name());
        }
    }
    
    // 2. Route Analysis  
    // Files in routes/ directory → route handlers
    // [param] syntax → dynamic segments
    // _middleware.ts → middleware chain
    fn analyze_routes(&mut self, module: &Module, path: &Path) {
        if path.to_string().contains("routes/") {
            if let Some(handler) = extract_handler(module) {
                self.route_handlers.push(handler);
            }
        }
    }
    
    // 3. Hook Tracking
    // Track useState, useEffect, etc. for proper code generation
    fn track_hooks(&mut self, expr: &Expr) {
        match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident { name } = callee.as_ref() {
                    if name.starts_with("use") {
                        self.hook_usages.push(HookUsage {
                            name: name.clone(),
                            args: args.clone(),
                        });
                    }
                }
            }
            _ => walk_expr(expr, |e| self.track_hooks(e)),
        }
    }
    
    // 4. Type Inference (limited)
    // Infer types for better Rust codegen
    fn infer_type(&mut self, expr: &Expr) -> Type {
        // ...
    }
}
```

### 4.4 Code Generation

The code generator transforms HIR to Rust:

```rust
// Key transformations:

// 1. Hooks → Runtime calls
useState(initial) → use_state(|| initial)
useEffect(fn, deps) → use_effect(fn, deps)
useRef(initial) → use_ref(|| initial)

// 2. JSX → html! macro
<div class="foo">{count}</div> 
→ html!(<div class_name="foo">{count.to_string()}</div>)

// 3. Event handlers → on_* attributes
onClick={handler} → on_click(handler)
onInput={handler} → on_input(handler)

// 4. Type mapping
string → String
number → f64
boolean → bool
null/undefined → Option<T>
Array<T> → Vec<T>

// 5. Arrow functions → closures
(x: number) => x + 1 → |x: f64| x + 1

// 6. Optional chaining → match/if let
obj?.prop?.nested → 
    if let Some(o) = obj { o.prop.nested } else { None }
```

---

## 5. Runtime System

### 5.1 Signal System (Fine-Grained Reactivity)

Based on Leptos-inspired design for optimal performance:

```rust
// From src/runtime/signals.rs
pub struct Signal<T: Clone> {
    value: Arc<RwLock<T>>,
}

impl<T: Clone> Signal<T> {
    pub fn new(value: T) -> Self;
    pub fn get(&self) -> T;
    pub fn set(&self, value: T);
    pub fn update<F>(&self, f: F) where F: FnOnce(&mut T);
}

// Example usage:
let count = Signal::new(0);
count.set(1);
count.update(|c| *c += 1);
```

### 5.2 Hooks Implementation

```rust
// From src/runtime/hooks.rs
pub fn use_state<T, F>(initial: F) -> (T, Box<dyn Fn(T) + Send + Sync>)
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> T + Clone + Send + Sync + 'static;

pub fn use_effect<F, D>(callback: F, deps: D)
where
    F: FnOnce() -> Option<EffectCleanup>;

pub fn use_memo<T, F, D>(factory: F, deps: &[D]) -> T;
pub fn use_callback<F, D>(callback: F, deps: &[D]) -> F;
pub fn use_ref<T, F>(initial: F) -> Ref<T>;
pub fn use_context<T>(context: &Context<T>) -> T;
pub fn use_id() -> String;
```

### 5.3 Virtual DOM / Rendering

```rust
// From src/runtime/vdom.rs
pub enum VNode {
    Element { tag: String, attrs: HashMap<String, AttrValue>, events: HashMap<String, EventHandler>, children: Vec<VNode>, key: Option<Key> },
    Component { name: String, props: HashMap<String, serde_json::Value>, children: Vec<VNode>, key: Option<Key> },
    Text { value: String },
    Fragment(Vec<VNode>),
    Empty,
}

pub trait Render {
    fn render_to_html(&self) -> String;
}
```

---

## 6. Islands Architecture

### 6.1 Island Detection

Islands are identified by:
1. **Location**: Files in `islands/` directory
2. **Export**: Default export of a function component
3. **Import**: Imported in routes using standard import syntax

### 6.2 Island Serialization

Only JSON-serializable props are supported:

```typescript
// ✅ Valid island props
<Counter initial={42} label={"Count"} />
<Counter initial={count} />  // where count is a primitive

// ❌ Invalid island props
<Counter onUpdate={handleUpdate} />  // Functions not serializable
<Counter date={new Date()} />       // Dates need adapter
```

### 6.3 Island Modes

| Mode | Trigger | Use Case |
|------|---------|----------|
| `eager` | Immediate | Forms, critical UI |
| `lazy` | IntersectionObserver | Below-the-fold content |
| `interaction` | First user interaction | Modals, tooltips |
| `visible` | MutationObserver | Dynamically added |

### 6.4 Server-Side Rendering

```
┌────────────────────────────────────────────────────────────────────┐
│ SSR Flow                                                           │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Request                                                           │
│     │                                                              │
│     ▼                                                              │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ Route Handler                                                 │    │
│  │ - Calls page component                                       │    │
│  │ - Islands output: props JSON + placeholder                    │    │
│  └────────────────────────────────────────────────────────────┘    │
│     │                                                              │
│     ▼                                                              │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ HTML Response                                               │    │
│  │ <div data-island="Counter" data-id="island-123">          │    │
│  │   <script type="application/x-runts-island">              │    │
│  │     { "initial": 42, "label": "Count" }                    │    │
│  │   </script>                                                │    │
│  │   <span>42</span>  ← SSR placeholder                       │    │
│  │ </div>                                                      │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                     │
└────────────────────────────────────────────────────────────────────┘
```

### 6.5 Client-Side Hydration

```
┌────────────────────────────────────────────────────────────────────┐
│ Hydration Flow                                                     │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Page Load                                                         │
│     │                                                              │
│     ▼                                                              │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ runts Runtime (~5KB)                                        │    │
│  │ - Find all [data-island] elements                          │    │
│  │ - Parse serialized props                                   │    │
│  │ - Register hydration callbacks                              │    │
│  └────────────────────────────────────────────────────────────┘    │
│     │                                                              │
│     ▼                                                              │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ Hydration Modes                                             │    │
│  │                                                             │    │
│  │ EAGER:     Hydrate immediately                              │    │
│  │ LAZY:      IntersectionObserver → hydrate on viewport       │    │
│  │ INTERACTION: First user interaction → hydrate                │    │
│  │ VISIBLE:   MutationObserver → hydrate on visibility          │    │
│  └────────────────────────────────────────────────────────────┘    │
│     │                                                              │
│     ▼                                                              │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ Interactive Island                                         │    │
│  │ - Connect signals to DOM                                   │    │
│  │ - Register event handlers                                   │    │
│  │ - Replace placeholder with live component                  │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                     │
└────────────────────────────────────────────────────────────────────┘
```

---

## 7. Development Mode

### 7.1 Architecture

Dev mode is **zero-compilation** — TypeScript is transpiled in-memory and executed via a Rust interpreter with JS-like semantics:

```
┌────────────────────────────────────────────────────────────────────┐
│ Development Server                                                  │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ File Watcher (notify crate)                                 │    │
│  │ - Watches routes/, islands/, components/                    │    │
│  │ - Debounces rapid changes (100ms)                          │    │
│  │ - Emits change events via broadcast channel                │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ Transpiler Cache (in-memory)                               │    │
│  │ - HIR for each module                                      │    │
│  │ - Incremental re-parse only changed files                   │    │
│  │ - Type info for completion                                 │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ HIR Interpreter (Rust)                                     │    │
│  │ - Execute HIR with JS-like semantics                       │    │
│  │ - Track signal dependencies                                │    │
│  │ - Render to HTML strings                                   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                      │
│                              ▼                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ WebSocket HMR (optional)                                   │    │
│  │ - Push updates to browser                                  │    │
│  │ - Show error overlay on failures                          │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                     │
└────────────────────────────────────────────────────────────────────┘
```

### 7.2 Hot Reload Flow

1. **File Change** → notify detects change
2. **Invalidate Cache** → mark changed file as dirty
3. **Incremental Parse** → re-parse only changed file + dependents
4. **HIR Update** → update cached AST
5. **Request Invalidation** → mark affected routes as needing re-render
6. **Browser Refresh** → fetch updated page (or WebSocket push)

### 7.3 Dev Server Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/_runts/hot` | WebSocket | HMR events |
| `/_runts/transpile` | POST | Manual transpile request |
| `/_runts/errors` | GET | Current compilation errors |
| `/_runts/state` | GET | Component tree state |

### 7.4 Dev vs Production

| Aspect | Dev Mode | Production |
|--------|----------|------------|
| Compilation | In-memory | Cargo build |
| Rust binary | No | Yes |
| Response time | ~5ms | ~1ms |
| Hot reload | Instant | N/A (restart) |
| Error messages | Detailed | Optimized |
| Source maps | Full | Optional |

---

## 8. Production Build

### 8.1 Build Pipeline

```
┌────────────────────────────────────────────────────────────────────┐
│ Production Build                                                    │
├────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Source Files                                                       │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐                 │
│  │routes/ │  │islands/ │  │components/│ │lib/    │                 │
│  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘               │
│       │            │            │            │                      │
│       └────────────┴─────┬──────┴────────────┘                      │
│                          ▼                                          │
│              ┌───────────────────────┐                              │
│              │ Transpiler (parallel) │                              │
│              │ - Thread pool        │                              │
│              │ - Error aggregation  │                              │
│              └───────────┬───────────┘                              │
│                          ▼                                          │
│              ┌───────────────────────┐                              │
│              │ Cargo Build           │                              │
│              │ - LTO                │                              │
│              │ - Single codegen unit │                              │
│              │ - Panic = abort       │                              │
│              └───────────┬───────────┘                              │
│                          ▼                                          │
│              ┌───────────────────────┐                              │
│              │ Binary Output         │                              │
│              │ ~2.5MB stripped       │                              │
│              │ Self-contained        │                              │
│              └───────────────────────┘                              │
│                                                                     │
└────────────────────────────────────────────────────────────────────┘
```

### 8.2 Build Configuration

```toml
[profile.release]
lto = true
codegen-units = 1
panic = "abort"
strip = true
opt-level = "z"  # Optimize for size
```

### 8.3 Output Binary Structure

```rust
// Binary contains:
// 1. Static assets (embedded)
//    - Static files bundled via include_bytes!
//    - Pre-rendered HTML templates
    
// 2. Route handlers (compiled Rust)
//    - Axum handlers
//    - Request/response types

// 3. Island runtime (minimal JS, ~5KB)
//    - Embedded in binary
//    - Served as static asset

// 4. Server runtime
//    - Axum HTTP server
//    - Signal system
//    - Hook context
```

---

## 9. Roadmap

### 9.1 MVP ✅ (v0.1-v0.2)

**Goal:** Core Fresh compatibility with working examples

| Feature | Status | Notes |
|---------|--------|-------|
| TSX Parser | ✅ Done | ~1700 lines, recursive descent |
| HIR | ✅ Done | Full expression coverage |
| Code Generation | ✅ Done | Components, hooks, JSX → html! |
| Signals | ✅ Done | Fine-grained reactivity |
| Islands | ✅ Done | Config, SSR, client hydration |
| Dev Server | ✅ Done | File watching, in-memory transpile |
| Production Build | ✅ Done | Transpile + Cargo + LTO |
| Route Generation | ✅ Done | GET/POST handlers, dynamic segments |
| Middleware | ✅ Done | Chain generation |
| Examples | ✅ Done | my-blog with full Fresh patterns |

### 9.2 Phase 1: Completeness (v0.3)

**Goal:** 95% Fresh API coverage

| Feature | Status | Priority |
|---------|--------|----------|
| All hooks | ✅ Done | Complete |
| Route handlers | ✅ Done | Complete |
| Middleware chain | ✅ Done | Complete |
| Layouts | 🟡 Partial | Basic support |
| Error boundaries | ⬜ TODO | Future |
| Static file serving | ✅ Done | Built-in |
| Client-side signals | ✅ Done | In client runtime |
| `_app.tsx` | ✅ Done | App wrapper |

### 9.3 Phase 2: Developer Experience (v0.4)

**Goal:** Excellent DX

| Feature | Status | Priority |
|---------|--------|----------|
| Error messages | 🟡 Partial | Improve diagnostics |
| Source maps | ⬜ TODO | Debug support |
| VSCode extension | ⬜ TODO | Future |
| HMR WebSocket | 🟡 Partial | Basic polling |
| CLI improvements | ⬜ TODO | Better help, colors |
| Type checking | ⬜ TODO | Integration with tsc |

### 9.4 Phase 3: Performance (v0.5)

**Goal:** Production-ready performance

| Feature | Status | Target |
|---------|--------|--------|
| Binary size | ✅ Done | <3MB |
| Cold start | ✅ Done | <10ms |
| Incremental compilation | 🟡 Partial | Cache implemented |
| Parallel transpilation | ⬜ TODO | Thread pool |
| Memory optimization | ⬜ TODO | Arena allocators |

### 9.5 Phase 4: Ecosystem (v0.6+)

**Goal:** Rich ecosystem

| Feature | Status |
|---------|--------|
| Database integrations | ⬜ TODO |
| Auth helpers | ⬜ TODO |
| SSR streaming | ⬜ TODO |
| Edge deployment | ⬜ TODO |
| WASM components | ⬜ TODO |

---

## 10. Performance Targets

### 10.1 Build Performance

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| Cold start (transpile) | <500ms | ~100ms | ✅ |
| Incremental change | <50ms | ~20ms | ✅ |
| Full build | <10s | ~18s | 🟡 |
| Memory usage | <200MB | ~150MB | ✅ |

### 10.2 Runtime Performance

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| Cold start | <10ms | ~5ms | ✅ |
| Binary size | <3MB | ~2.5MB | ✅ |
| Memory (idle) | <10MB | ~5-10MB | ✅ |
| Throughput | >50k req/s | TBD | - |

### 10.3 Developer Experience

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| Dev server start | <500ms | ~200ms | ✅ |
| Hot reload | <100ms | ~50ms | ✅ |
| TypeScript check | <200ms | ~100ms | 🟡 |

---

## 11. Trade-offs

### 11.1 Hand-Written Parser vs swc

**Decision:** Custom recursive descent parser instead of swc

| Factor | swc | Custom Parser |
|--------|-----|--------------|
| Dependencies | Heavy (many crates) | Zero |
| Speed | Very fast | Fast enough |
| Control | Limited | Full |
| Maintenance | External | Internal |
| Bundle size | +2MB | +0 |
| Our subset | Overkill | Just right |

**Conclusion:** Custom parser is 90% of the functionality in 10% of the complexity.

### 11.2 Signals vs Virtual DOM

**Decision:** Fine-grained signals instead of full VDOM diffing

| Factor | VDOM | Signals |
|--------|------|---------|
| Memory | Higher (tree copy) | Lower |
| CPU | Diff on every render | Direct update |
| Complexity | High | Moderate |
| Fresh compat | Hard | Easy |
| Fine-grained | No | Yes |
| Bundle size | Larger | Smaller |

**Conclusion:** Signals provide better performance and match Preact Signals API.

### 11.3 Minimal Client JS

**Decision:** ~5KB client runtime instead of full Preact

| Factor | Full Preact (~4KB) | runts (~5KB) |
|--------|-------------------|--------------|
| Features | All | Signals + events |
| Islands | Full hydration | Minimal hydration |
| DevTools | Yes | Future |
| SSR compat | Good | Excellent |

**Conclusion:** Most Fresh apps don't need full Preact client — signals + event handlers suffice.

### 11.4 No TypeScript Type Checking

**Decision:** Trust TypeScript annotations without verification

| Factor | Type Check | No Type Check |
|--------|------------|---------------|
| Speed | Slower | Faster |
| Accuracy | High | Depends on user |
| Rust interop | Better | Limited |
| Complexity | High | Low |

**Conclusion:** User is responsible for TS types; we use them as hints for Rust codegen.

### 11.5 Dev Mode Interpretation vs Compilation

**Decision:** Interpret HIR in dev mode, compile for production

| Factor | Interpret Dev | Compiled Dev |
|--------|--------------|-------------|
| Hot reload | Instant | Fast (incremental) |
| Complexity | High | Low |
| Debugging | Harder | Easier |
| Production match | Different | Same |

**Conclusion:** Interpretation enables instant hot reload; production compilation ensures native performance.

---

## 12. Migration Guide

### 12.1 From Deno Fresh

Most Fresh code works with minimal changes:

```typescript
// Fresh (Deno)
import { PageProps } from "$fresh/server.ts";

// runts
import { PageProps, HandlerContext } from "$fresh/server.ts";
```

### 12.2 Key Differences

| Fresh | runts | Notes |
|-------|-------|-------|
| `Request` | `Request` | Same API |
| `Response` | `Response` | Same API |
| `ctx.params` | `ctx.params` | Same |
| `ctx.render()` | Not needed | SSR automatic |
| Islands | Islands | Same behavior |

### 12.3 Unsupported Features

```typescript
// These need to be rewritten:

// 1. Class components → Function components
// 2. enum → const objects or as const
// 3. Dynamic imports → Static imports
// 4. eval() → Not supported
```

### 12.4 Props Serialization

Island props must be JSON-serializable:

```typescript
// ✅ Valid
<Counter initial={42} />
<Counter items={["a", "b"]} />

// ❌ Invalid
<Counter onClick={handler} />
<Counter data={someClass} />
```

---

## Appendix A: Error Codes

| Code | Meaning | Example |
|------|---------|---------|
| `E001` | Parse error | Unexpected token |
| `E002` | Unsupported feature | Class component |
| `E003` | Invalid island props | Function prop |
| `E004` | Import resolution failed | Missing file |
| `E005` | Type conversion error | Complex generic |
| `W001` | Implicit any type | Missing annotation |
| `W002` | Unused import | Import not used |

---

## Appendix B: Configuration Schema

```json
{
  "$schema": "https://runts.dev/schema.json",
  "name": "my-app",
  "version": "0.1.0",
  
  "server": {
    "host": "0.0.0.0",
    "port": 8000,
    "https": false
  },
  
  "routes": {
    "dir": "routes",
    "layouts": true,
    "middleware": true
  },
  
  "islands": {
    "dir": "islands",
    "hydration": {
      "default": "lazy",
      "modes": ["eager", "lazy", "interaction", "visible"]
    }
  },
  
  "components": {
    "dir": "components"
  },
  
  "static": {
    "dir": "static",
    "prefix": "/static"
  },
  
  "build": {
    "target": null,
    "release": {
      "lto": true,
      "optLevel": "z",
      "codegenUnits": 1
    }
  },
  
  "dev": {
    "hotReload": true,
    "sourceMaps": true,
    "watchDirs": ["routes", "islands", "components", "lib"]
  }
}
```

---

**End of Specification**
