# runts Technical Specification

## Version 0.2.0 — Implementation Blueprint

**Status:** Implementation Draft  
**Last Updated:** 2025-05-26

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Supported TypeScript/TSX Subset](#2-supported-typescripttsx-subset)
3. [Transpilation Pipeline](#3-transpilation-pipeline)
4. [Runtime Architecture](#4-runtime-architecture)
5. [Islands Architecture](#5-islands-architecture)
6. [Development Mode](#6-development-mode)
7. [Production Build](#7-production-build)
8. [Implementation Roadmap](#8-implementation-roadmap)
9. [Performance Targets](#9-performance-targets)
10. [Error Handling](#10-error-handling)

---

## 1. Executive Summary

**runts** transforms Fresh/Preact TypeScript/TSX into native Rust binaries with zero external JS runtimes.

### Design Philosophy

1. **Fresh Compatibility First** — 95%+ API parity with Fresh framework
2. **Minimal TS Subset** — Ruthlessly exclude rarely-used features
3. **Pure Rust Runtime** — No V8, Deno, or WASM JS engines
4. **Fine-Grained Reactivity** — Signals over Virtual DOM

### Core Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        runts CLI                                      │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Source Files                                                       │
│  routes/*.tsx  +  islands/*.tsx  +  components/*.tsx               │
│           │                    │                    │               │
│           ▼                    ▼                    ▼               │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    PARSER LAYER                              │    │
│  │  • Recursive descent TSX parser (~57KB)                   │    │
│  │  • Produces HIR (High-Level IR)                          │    │
│  │  • ~10x faster than swc for our subset                   │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    ANALYZER LAYER                           │    │
│  │  • Island detection (islands/ directory)                   │    │
│  │  • Route analysis (routes/ + [param].tsx)                 │    │
│  │  • Hook tracking & validation                             │    │
│  │  • Signal dependency graph                                │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    CODE GENERATOR                           │    │
│  │  • HIR → Rust source code                                 │    │
│  │  • Components → #[component] proc macros                 │    │
│  │  • Hooks → runtime::hook_name() calls                    │    │
│  │  • JSX → html! macro invocations                         │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    RUNTIME LAYER                            │    │
│  │  • runts-lib: Rust signal system, hooks, VDOM             │    │
│  │  • runts-client: Minimal JS (~12KB) for island hydration  │    │
│  │  • Axum/Tower: HTTP server framework                      │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    OUTPUT                                   │    │
│  │  • Binary: ~500KB - 2MB (stripped release)                │    │
│  │  • Generated Rust: target/runts/gen/                       │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 2. Supported TypeScript/TSX Subset

### 2.1 Core Syntax (FULL SUPPORT)

| Feature | Syntax | Notes |
|---------|--------|-------|
| Type annotations | `let x: number = 5` | All positions |
| Interface declarations | `interface Foo { ... }` | Extends, index signatures |
| Type aliases | `type Foo = ...` | Unions, intersections, mapped types |
| Generic types | `function f<T>(x: T): T` | Constraints, inference |
| JSX/TSX | `<div>{expr}</div>` | Elements, fragments, expressions |
| Arrow functions | `const f = () => {}` | Multi-statement bodies |
| Async/await | `async function f() {}` | Top-level and nested |
| Destructuring | `const { a, b } = obj` | Objects, arrays, nested, default |
| Spread operator | `...obj`, `[...arr]` | Objects, arrays, JSX props |
| Template literals | `` `hello ${x}` `` | With expressions |
| Optional chaining | `obj?.prop?.field` | `?.` and `?.[` |
| Nullish coalescing | `x ?? default` | Nested, with other operators |
| Import/export | `import { x } from 'y'` | Named, default, re-exports |
| Conditional types | `T extends U ? A : B` | Basic support |

### 2.2 Preact Hooks (FULL SUPPORT)

| Hook | Signature | Notes |
|------|-----------|-------|
| `useState` | `useState<T>(initial: T \| (() => T))` | Lazy initializer |
| `useEffect` | `useEffect(() => cleanup, deps)` | Cleanup functions |
| `useRef` | `useRef<T>(initial: T)` | Mutable ref, no re-render |
| `useMemo` | `useMemo(() => value, deps)` | Dependency tracking |
| `useCallback` | `useCallback(fn, deps)` | Memoized callback |
| `useReducer` | `useReducer(reducer, initial, init?)` | Complex state |
| `useContext` | `useContext(Context)` | Provider required |
| `useLayoutEffect` | `useLayoutEffect(() => cleanup, deps)` | Sync with paint |
| `useId` | `useId(): string` | Stable ID generation |
| `useDebugValue` | `useDebugValue(x)` | No-op in production |

### 2.3 Preact Signals (FULL SUPPORT)

| API | Signature | Notes |
|-----|-----------|-------|
| `signal` | `signal<T>(initial: T)` | Reactive state container |
| `computed` | `computed<T>(fn: () => T)` | Derived values |
| `effect` | `effect(fn: () => void)` | Side effects |
| `batch` | `batch(fn: () => void)` | Group updates |
| `useSignal` | `useSignal<T>(initial: T)` | Hook variant |
| `useComputed` | `useComputed<T>(fn: () => T)` | Hook variant |
| `signalEffect` | `signalEffect(fn: () => void)` | Effect with auto-cleanup |

### 2.4 Fresh-Specific APIs (FULL SUPPORT)

| API | Type | Notes |
|-----|------|-------|
| `PageProps` | Interface | Route params + URL |
| `HandlerContext` | Type | Request context |
| `Handler` object | `{ GET?: Handler, POST?: Handler }` | HTTP method handlers |
| `IS_BROWSER` | `boolean` | Runtime detection |
| `UnknownPageProps` | Interface | 404 route props |
| `_middleware.ts` | Module | Request middleware |
| `_layout.tsx` | Component | Layout wrapper |
| `_app.tsx` | Component | App wrapper |

### 2.5 File-Based Routing (FULL SUPPORT)

```
routes/
├── index.tsx                    → GET /
├── about.tsx                    → GET /about
├── contact.tsx                  → GET /contact
├── blog/
│   ├── index.tsx               → GET /blog
│   ├── _layout.tsx             → Blog layout wrapper
│   ├── [slug].tsx              → GET /blog/:slug
│   └── _middleware.ts          → Blog-specific middleware
├── api/
│   └── [...path].tsx           → Catch-all: GET /api/*, POST /api/*, etc.
├── products/
│   └── [category]/
│       ├── index.tsx           → GET /products/:category
│       └── [id].tsx            → GET /products/:category/:id
└── _middleware.ts             → Global middleware
```

### 2.6 Deliberate Exclusions

```typescript
// ❌ Class components
class MyComponent extends Component { }
class Foo extends React.Component { }

// ❌ Legacy React APIs
createContext(defaultValue)        // Use createContext with Provider
React.memo(Component)             // Use signals instead
React.forwardRef((props, ref) =>) // Use callback refs
React.Suspense + lazy()          // Use islands architecture
createPortal(<Content>, domNode)  // Server-side rendering only
React.StrictMode                  // Dev-only, ignored in prod

// ❌ TypeScript features
namespace MyNamespace { }         // Use ES modules
enum Color { Red, Green }        // Use 'as const' objects
declare module 'x' { }          // Use .d.ts files
namespace.JsExtensions()         // Legacy JSX transform
decorators (stage 3)             // Use function wrappers
parameter decorators              // Not supported
abstract class                   // Use interface + factory

// ❌ JavaScript features
eval()                           // Security risk
new Function(...)                // Security risk
with statement                   // Deprecated
label statements                 // Poor code quality
do-while (can rewrite as while)  // Not needed

// ❌ Complex patterns
Generator functions (yield)      // Use async/await
Iterator protocols               // Not needed for Fresh
WeakMap/WeakSet serialization    // Can't serialize
Proxy traps                     // Runtime-only
Symbol.toStringTag              // Not serializable

// ❌ Browser-only APIs (require polyfills)
window.localStorage              // Use lib/ with polyfill
document.cookie                  // Use cookies crate
fetch                            // Use reqwest crate
WebSocket                        // Use tokio-tungstenite
```

### 2.7 Type Mapping

| TypeScript | Rust | Notes |
|------------|------|-------|
| `string` | `String` | UTF-8 owned string |
| `number` | `f64` | IEEE 754 double |
| `boolean` | `bool` | 1 byte |
| `null` | `Option<T>` | Null → None |
| `undefined` | `Option<T>` | Undefined → None |
| `any` | `serde_json::Value` | Dynamic JSON |
| `unknown` | `serde_json::Value` | Dynamic JSON |
| `void` | `()` | Unit type |
| `never` | `!` | Never returns |
| `object` | `serde_json::Value` | Dynamic object |
| `Array<T>` | `Vec<T>` | Growable vector |
| `ReadonlyArray<T>` | `&[T]` | Borrowed slice |
| `Record<K,V>` | `HashMap<K,V>` | String keys |
| `Promise<T>` | `JoinHandle<T>` | Tokio async |
| `Function` | `Box<dyn Fn() + Send + Sync>` | Callable |
| `Date` | `chrono::DateTime` | With serialization |
| `Map<K,V>` | `std::collections::HashMap` | Hash map |
| `Set<T>` | `std::collections::HashSet` | Hash set |
| `RegExp` | `regex::Regex` | With compile |
| `Partial<T>` | `Option<T>` per field | Optional fields |
| `Required<T>` | `T` | All fields required |
| `Readonly<T>` | `T` (const) | Immutability |

---

## 3. Transpilation Pipeline

### 3.1 Parser (Recursive Descent)

**Location:** `src/transpile/parser.rs` (~57KB)

The parser is hand-written for zero dependencies and maximum control.

#### Grammar (Simplified EBNF)

```ebnf
Module           := ModuleItem* EOF
ModuleItem       := Import | Export | Decl

Import           := "import" ImportSpec "from" String ("as" Ident)? ";"
Export           := "export" (ExportSpec | Decl)
Decl             := Function | Variable | Type | Interface | Enum

Function         := ("async")? "function" Ident TypeParams? Params Type? Block
Arrow            := Params "=>" (Block | Expr)
Params           := "(" ParamList? ")"
Param            := Pattern Type? ("=" Expr)?

Type             := UnionType
UnionType        := IntersectType ("|" IntersectType)*
IntersectType    := PrimaryType ("&" PrimaryType)*
PrimaryType      := "(" Type ")" | TypeRef | PrimType | FnType | ObjType | ArrType

TypeRef          := Ident TypeArgs?
TypeArgs         := "<" TypeList? ">"
PrimType         := "string" | "number" | "boolean" | "void" | "null" | "undefined" | "any" | "unknown" | "never" | "object"
FnType           := "(" ParamList? ")" "=>" Type
ObjType          := "{" TypeMemberList? "}"
ArrType          := PrimaryType "[" "]"

Pattern          := Ident | ObjPat | ArrPat | RestPat
ObjPat           := "{" (FieldPat ("," FieldPat)* ","?)? "}"
FieldPat         := Ident ("as" Ident)? Type? ("=" Expr)?
ArrPat           := "[" Pattern ("," Pattern)* ","? "]"
RestPat          := "..." Ident

Expr             := Assignment
Assignment       := Cond ("=" | "+=" | "-=" | ...)* Cond
Cond             := LogicalOr ("?" Expr ":" Expr)?
LogicalOr        := LogicalAnd ("||" LogicalAnd)*
LogicalAnd       := BitOr ("&&" BitOr)*
BitOr            := BitXor ("|" BitXor)*
BitXor           := BitAnd ("^" BitAnd)*
BitAnd           := Equality ("&" Equality)*
Equality         := Comparison (("==" | "!=" | "===" | "!==") Comparison)*
Comparison       := Range ((">" | "<" | ">=" | "<=") Range)*
Range            := Shift (".." Shift)?
Shift            := Add (("<<" | ">>") Add)*
Add              := Mul (("+" | "-") Mul)*
Mul              := Unary (("*" | "/" | "%") Unary)*
Unary            := ("!" | "-" | "+" | "typeof" | "void" | "~") Unary | Call
Call             := Member ("(" Arguments? ")")*
Member           := Primary ("." Ident | "[" Expr "]")*
Primary          := Ident | Lit | Template | Lambda | JSX | "(" Expr ")"

Lambda           := Params "=>" (Block | Expr)
JSX              := "<" JSXName JSXAttrs? ("/>" | ">" JSXChildren "</" JSXName ">")
JSXName          := Ident ("." Ident)* | Ident ":" Ident
JSXAttrs         := JSXAttr+
JSXAttr          := JSXName ("=" JSXAttrValue)? | "..." Expr
JSXAttrValue     := String | "{" Expr "}"
JSXChildren     := (JSX | "{" Expr "}" | Text)*
```

#### Parser Traits

```rust
impl Parser {
    // Core methods
    pub fn parse_source(&mut self, source: &str) -> Result<Module>
    pub fn parse_file(&mut self, path: &PathBuf) -> Result<Module>
    
    // Module items
    fn parse_import(&mut self) -> Result<Option<Import>>
    fn parse_export(&mut self) -> Result<Option<Export>>
    fn parse_decl(&mut self) -> Result<Option<Decl>>
    
    // Declarations
    fn parse_function(&mut self) -> Result<Option<FunctionDecl>>
    fn parse_variable_decl(&mut self) -> Result<Option<VariableDecl>>
    fn parse_type_alias(&mut self) -> Result<Option<TypeDecl>>
    fn parse_interface(&mut self) -> Result<Option<TypeDecl>>
    
    // Types
    fn parse_type(&mut self) -> Result<Type>
    fn parse_union_type(&mut self) -> Result<Type>
    fn parse_fn_type(&mut self) -> Result<Type>
    fn parse_object_type(&mut self) -> Result<Type>
    
    // Expressions
    fn parse_expr(&mut self) -> Result<Expr>
    fn parse_assignment(&mut self) -> Result<Expr>
    fn parse_conditional(&mut self) -> Result<Expr>
    fn parse_logical_or(&mut self) -> Result<Expr>
    fn parse_call(&mut self) -> Result<Expr>
    fn parse_member(&mut self) -> Result<Expr>
    
    // JSX
    fn parse_jsx(&mut self) -> Result<Expr>
    fn parse_jsx_element(&mut self, name: JSXName) -> Result<JSXExpr>
    fn parse_jsx_attrs(&mut self) -> Result<Vec<JSXAttr>>
    fn parse_jsx_children(&mut self) -> Result<Vec<JSXChild>>
    
    // Utilities
    fn skip_ws_and_comments(&mut self)
    fn expect(&mut self, c: char) -> Result<()>
    fn current(&self) -> char
    fn peek(&self) -> char
    fn is_at_end(&self) -> bool
    fn advance(&mut self) -> char
}
```

### 3.2 High-Level IR (HIR)

**Location:** `src/transpile/hir.rs`

The HIR normalizes the AST into a form suitable for code generation.

#### Core Types

```rust
// Module representation
pub struct Module {
    pub source: String,
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDecl>,
}

// Module items
pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
    Class(ClassDecl),
}

// Function declaration
pub struct FunctionDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Option<Block>,
    pub is_async: bool,
    pub is_component: bool,
    pub is_handler: bool,
}

// Expression variants
pub enum Expr {
    Ident { name: String },
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Undefined,
    Bin { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, arg: Box<Expr> },
    Logical { op: LogicalOp, left: Box<Expr>, right: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    Member { object: Box<Expr>, property: Box<Expr>, computed: bool },
    Object { props: Vec<ObjectProp> },
    Array { elems: Vec<Option<Expr>> },
    Arrow { params: Vec<Param>, body: Box<Stmt> },
    Function { decl: Box<FunctionDecl> },
    Assign { left: Box<Expr>, right: Box<Expr> },
    JSX(JSXExpr),
    // ... more variants
}

// JSX representation
pub struct JSXExpr {
    pub opening: JSXOpening,
    pub children: Vec<JSXChild>,
}

pub struct JSXOpening {
    pub name: JSXName,
    pub attrs: Vec<JSXAttr>,
    pub self_closing: bool,
}

pub enum JSXName {
    Ident(String),                    // <div>
    Member { object: String, property: String },  // <Icon.Name>
    Namespaced { ns: String, name: String },     // <svg:rect>
    Dynamic(Box<Expr>),              // <{tag}>
}
```

### 3.3 Semantic Analyzer

**Location:** `src/transpile/analyzer.rs`

The analyzer performs type inference, island detection, and route analysis.

```rust
pub struct Analyzer {
    // Scope management
    scopes: Vec<Scope>,
    
    // Island detection
    pub islands: Vec<IslandInfo>,
    
    // Route detection  
    pub routes: Vec<RouteInfo>,
    
    // Hook tracking
    hook_calls: Vec<HookCall>,
    
    // Signal tracking
    signal_refs: Vec<SignalRef>,
    
    // Errors
    errors: Vec<AnalyzerError>,
}

impl Analyzer {
    pub fn analyze(&mut self, module: &Module) -> Result<(), Vec<AnalyzerError>> {
        self.enter_scope(Scope::global());
        
        for item in &module.items {
            self.analyze_module_item(item)?;
        }
        
        self.exit_scope();
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }
    
    // Island detection
    fn detect_island(&mut self, path: &Path, decl: &FunctionDecl) {
        // Files in islands/ directory are islands
        // Default exports with hooks are islands
    }
    
    // Route detection
    fn detect_route(&mut self, path: &Path, module: &Module) {
        // Parse routes/*.tsx paths to route patterns
        // Detect [param] → dynamic segments
        // Extract handler exports (GET, POST, etc.)
    }
    
    // Hook validation
    fn validate_hook_call(&mut self, call: &CallExpr) {
        // Rules of hooks: only call at top level
        // Track hook order for state management
    }
}
```

### 3.4 Code Generator

**Location:** `src/transpile/codegen.rs`

Transforms HIR into idiomatic Rust source code.

#### Component Transformation

```typescript
// Input: Fresh/Preact TSX
interface CounterProps {
  initial?: number;
}

export default function Counter({ initial = 0 }: CounterProps) {
  const [count, setCount] = useState(initial);
  
  return (
    <div class="counter">
      <p>{count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

```rust
// Output: Generated Rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CounterProps {
    pub initial: Option<f64>,
}

impl Default for CounterProps {
    fn default() -> Self {
        Self { initial: None }
    }
}

#[component]
pub fn counter(props: CounterProps) -> VNode {
    let initial = props.initial.unwrap_or(0.0);
    let (count, set_count) = use_state(|| initial);
    
    html! {
        <div class="counter">
            <p>{ count.to_string() }</p>
            <button on_click={ move |_| set_count(count + 1.0) }>+</button>
        </div>
    }
}

impl Component for Counter {
    fn render(&self) -> VNode {
        counter(self.props.clone())
    }
}
```

#### Route Handler Transformation

```typescript
// Input: routes/blog/[slug].tsx
import { PageProps } from "$fresh/server.ts";

interface Props extends PageProps {
  data: { title: string; content: string };
}

export default function BlogPost({ params, data }: Props) {
  return (
    <article>
      <h1>{data.title}</h1>
      <div>{data.content}</div>
    </article>
  );
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext) {
    const post = await getPost(ctx.params.slug);
    return ctx.render({ title: post.title, content: post.content });
  }
};
```

```rust
// Output: Generated Rust
use runts_lib::runtime::prelude::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct BlogPostProps {
    pub params: RouteParams,
    pub data: BlogData,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BlogData {
    pub title: String,
    pub content: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RouteParams {
    pub slug: String,
}

pub async fn blog_slug_handler(
    req: Request,
    params: RouteParams,
) -> Response {
    let post = get_post(&params.slug).await;
    let data = BlogData {
        title: post.title,
        content: post.content,
    };
    html! {
        <article>
            <h1>{ data.title }</h1>
            <div>{ data.content }</div>
        </article>
    }.render()
}
```

#### JSX to html! Transformation

| JSX | html! |
|-----|-------|
| `<div class="x">` | `<div class_name="x">` |
| `<div onClick={h}>` | `<div on_click={h}>` |
| `<div>{expr}</div>` | `<div>{ expr.to_string() }</div>` |
| `<div>{a && b}</div>` | `<div>{ if a { Some(b) } else { None } }</div>` |
| `<div>{a || b}</div>` | `<div>{ if a.is_empty() { Some(b) } else { Some(a) } }</div>` |
| `<Fragment>` | `html!(<>)` |
| `<></>` | `html!(<>)` |

---

## 4. Runtime Architecture

### 4.1 Signal System

**Location:** `src/runtime/signals.rs`

Fine-grained reactivity inspired by Leptos/SolidJS.

```rust
/// Reactive signal container
pub struct Signal<T: Clone> {
    value: Arc<RwLock<T>>,
    subscribers: Arc<RwLock<HashSet<usize>>>,
    equals: Arc<dyn Fn(&T, &T) -> bool + Send + Sync>,
}

impl<T: Clone> Signal<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(value)),
            subscribers: Arc::new(RwLock::new(HashSet::new())),
            equals: Arc::new(|a, b| a == b),
        }
    }
    
    pub fn with_equals<F>(value: T, equals: F) -> Self 
    where F: Fn(&T, &T) -> bool + Send + Sync + 'static {
        Self {
            value: Arc::new(RwLock::new(value)),
            subscribers: Arc::new(RwLock::new(HashSet::new())),
            equals: Arc::new(equals),
        }
    }
    
    /// Get current value (clones)
    pub fn get(&self) -> T {
        self.value.read().clone()
    }
    
    /// Set new value, notify subscribers if changed
    pub fn set(&self, value: T) {
        let changed = {
            let current = self.value.read();
            !self.equals(&current, &value)
        };
        
        if changed {
            *self.value.write() = value;
            self.notify();
        }
    }
    
    /// Update with a function
    pub fn update<F>(&self, f: F) 
    where F: FnOnce(&mut T) {
        let mut value = self.value.write();
        f(&mut value);
        drop(value);
        self.notify();
    }
    
    /// Peek without tracking dependency
    pub fn peek(&self) -> T {
        self.value.read().clone()
    }
}

fn notify(&self) {
    let subs = self.subscribers.read().clone();
    for id in subs {
        // Queue subscriber notifications
    }
}
```

### 4.2 Hooks Implementation

**Location:** `src/runtime/hooks.rs`

React/Preact-compatible hooks with Rust ownership semantics.

```rust
/// useState hook - creates reactive state
pub fn use_state<T, F>(initial: F) -> (T, Box<dyn Fn(T) + Send + Sync>)
where
    T: Clone + Send + Sync + 'static,
    F: FnOnce() -> T + Clone + Send + Sync + 'static,
{
    let component_ctx = use_component_context::<T>();
    let state = component_ctx.get_or_init_state(initial);
    
    let getter = state.get();
    let setter: Box<dyn Fn(T) + Send + Sync> = Box::new(move |new_value| {
        state.set(new_value);
        component_ctx.request_render();
    });
    
    (getter, setter)
}

/// useEffect hook - runs side effects
pub fn use_effect<F, D>(callback: F, deps: D)
where
    F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
    D: AsRef<[usize]> + 'static,
{
    let component_ctx = use_component_context::<()>();
    
    // Schedule effect for after render
    component_ctx.schedule_effect(callback, deps);
}

/// useRef hook - mutable reference without re-render
pub fn use_ref<T, F>(initial: F) -> Ref<T>
where
    T: Clone + 'static,
    F: FnOnce() -> T,
{
    let component_ctx = use_component_context::<RefInner<T>>();
    component_ctx.get_or_init_ref(initial)
}

/// useMemo hook - memoized computation
pub fn use_memo<T, F, D>(factory: F, deps: &[D]) -> T
where
    T: Clone + 'static,
    F: FnOnce() -> T,
    D: Hash + Eq + 'static,
{
    let component_ctx = use_component_context::<MemoEntry<T>>();
    let entry = component_ctx.get_or_init_memo(factory, deps);
    entry.get().expect("memo should be computed")
}
```

### 4.3 Component System

**Location:** `src/runtime/component.rs`

```rust
/// Component trait - implemented by all components
pub trait Component: 'static {
    type Props: Clone + Send + Sync + Serialize + for<'de> Deserialize<'de>;
    
    fn render(props: Self::Props) -> VNode;
}

/// Component context for hooks
pub struct ComponentContext {
    // State storage per hook
    states: RefCell<Vec<Box<dyn Any>>>,
    
    // Effect queue
    effects: RefCell<Vec<EffectEntry>>,
    
    // Render request flag
    needs_render: Cell<bool>,
    
    // Memo cache
    memos: RefCell<HashMap<usize, MemoEntry<Box<dyn Any>>>>,
}

impl ComponentContext {
    pub fn new() -> Self;
    pub fn get_or_init_state<F, T>(&self, init: F) -> Arc<State<T>>
    where F: FnOnce() -> T, T: Clone;
    
    pub fn schedule_effect<F, D>(&self, callback: F, deps: D)
    where F: FnOnce() -> Option<EffectCleanup> + Send + Sync + 'static,
          D: AsRef<[usize]> + 'static;
    
    pub fn request_render(&self);
}
```

### 4.4 html! Macro

**Location:** `crates/runts-macros/src/html.rs`

```rust
#[proc_macro]
pub fn html(input: TokenStream) -> TokenStream {
    let html_tree = parse_html(input);
    let rust_code = generate_html(html_tree);
    rust_code.into()
}

// Example transformation:
// <div class="container">{count}</div>
// becomes:
// html_element("div")
//     .attr("class", "container")
//     .child(html_text(&count.to_string()))
//     .build()
```

---

## 5. Islands Architecture

### 5.1 Island Definition

**Location:** `src/runtime/islands.rs`

Islands are interactive components that hydrate on the client.

```rust
/// Island configuration
#[derive(Debug, Clone)]
pub struct IslandConfig {
    /// Component name
    pub name: String,
    
    /// Unique instance ID
    pub id: String,
    
    /// Serialized props (JSON)
    pub props: serde_json::Value,
    
    /// Hydration mode
    pub mode: IslandMode,
    
    /// Uses signals
    pub has_signals: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandMode {
    /// Hydrate immediately
    Eager,
    
    /// Hydrate on viewport entry
    Lazy,
    
    /// Hydrate on first interaction
    Interaction,
    
    /// Hydrate when visible
    Visible,
}
```

### 5.2 Island Container HTML

```html
<!-- Server-rendered island -->
<div data-island="Counter" 
     data-id="island-a1b2c3d4" 
     data-mode="lazy"
     data-has-signals="true">
  <script type="application/x-runts-island">
    {"initial": 42, "step": 1}
  </script>
  <span class="count">42</span>
</div>
```

### 5.3 Client Hydration

**Location:** `crates/runts-client/src/runtime.ts`

```typescript
export enum IslandMode {
  Eager = 'eager',
  Lazy = 'lazy',
  Interaction = 'interaction',
  Visible = 'visible'
}

interface IslandInstance {
  id: string;
  name: string;
  props: any;
  mode: IslandMode;
  state: 'pending' | 'hydrating' | 'hydrated' | 'error';
  element: HTMLElement | null;
}

// Discover all islands on page
function discoverIslands(): void {
  document.querySelectorAll('[data-island]').forEach(el => {
    const config = parseIslandConfig(el);
    registerIsland(config);
  });
}

// Hydrate based on mode
export async function hydrateIsland(id: string): Promise<void> {
  const island = getIsland(id);
  if (!island) return;
  
  island.state = 'hydrating';
  
  // Load component bundle
  const bundle = await loadIslandBundle(island.name);
  
  // Initialize component with props
  bundle.mount(island.id, island.props);
  
  island.state = 'hydrated';
}
```

---

## 6. Development Mode

### 6.1 Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│ Development Server                                                  │
├────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │ File Watcher (notify crate)                                 │    │
│  │ • Watches: routes/, islands/, components/, lib/            │    │
│  │ • Debounce: 50ms for rapid changes                         │    │
│  │ • On change: emit event to watcher                         │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │ Transpiler Cache (in-memory)                                │    │
│  │ • Module cache: path → HIR                                 │    │
│  │ • Incremental: only re-parse changed file                 │    │
│  │ • Dependency graph: file → dependents                      │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │ TypeScript Interpreter (Rust-based)                        │    │
│  │ • Execute HIR with JS-like semantics                       │    │
│  │ • Hook context management                                  │    │
│  │ • Signal tracking                                          │    │
│  │ • Component rendering                                      │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                              │                                       │
│                              ▼                                       │
│  ┌──────────────────────────────────────────────────────────────┐    │
│  │ Axum Dev Server                                            │    │
│  │ • Serves routes with dynamic rendering                     │    │
│  │ • WebSocket for HMR                                        │    │
│  │ • File server for static assets                            │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                                                                      │
└────────────────────────────────────────────────────────────────────┘
```

### 6.2 Hot Module Replacement

```rust
// WebSocket HMR endpoint
async fn handle_hmr(ws: WebSocket, path: &str) {
    let mut client = ws.accept().await.unwrap();
    
    // Watch for file changes
    let watcher = FileWatcher::new(&["routes", "islands", "components"]);
    
    loop {
        tokio::select! {
            // File changed → transpile and send update
            changed = watcher.changed() => {
                let module = changed.path.parse().unwrap();
                let delta = Delta {
                    kind: DeltaKind::ModuleUpdate,
                    path: changed.path,
                    // HIR diff or full module
                    content: serialize(&module),
                };
                client.send_json(&delta).await;
            }
            
            // Client message (request full reload, etc.)
            msg = client.recv() => {
                handle_client_message(msg);
            }
        }
    }
}
```

### 6.3 Dev Mode Execution Flow

1. **Request** → `GET /blog/my-post`
2. **Route match** → `routes/blog/[slug].tsx`
3. **Parse (cached)** → Get HIR from cache
4. **Execute** → Interpret HIR with hook context
5. **Render** → Generate HTML response
6. **Response** → Send HTML + HMR script

---

## 7. Production Build

### 7.1 Build Pipeline

```bash
┌────────────────────────────────────────────────────────────────────┐
│ Production Build                                                    │
├────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Source Files                                                       │
│  routes/*.tsx  +  islands/*.tsx  +  components/*.tsx               │
│           │                    │                    │               │
│           └────────────┬────────┴─────────┬──────────┘              │
│                        ▼                   ▼                          │
│              ┌────────────────────┐  ┌────────────────────┐         │
│              │  Transpile (dev)  │  │  Transpile (prod)   │         │
│              │  • In-memory      │  │  • Parallel         │         │
│              │  • Fast reload    │  │  • Thread pool      │         │
│              └────────┬─────────┘  └──────────┬───────────┘         │
│                       │                        │                     │
│                       └───────────┬────────────┘                     │
│                                   ▼                                  │
│                        ┌─────────────────────┐                       │
│                        │  Generated Rust      │                       │
│                        │  target/runts/gen/  │                       │
│                        └──────────┬──────────┘                       │
│                                   │                                  │
│                                   ▼                                  │
│                        ┌─────────────────────┐                       │
│                        │  Cargo Build        │                       │
│                        │  • LTO               │                       │
│                        │  • Single codegen    │                       │
│                        │  • opt-level=z       │                       │
│                        └──────────┬──────────┘                       │
│                                   │                                  │
│                                   ▼                                  │
│                        ┌─────────────────────┐                       │
│                        │  Binary Output      │                       │
│                        │  ~500KB - 2MB       │                       │
│                        └─────────────────────┘                       │
│                                                                      │
└────────────────────────────────────────────────────────────────────┘
```

### 7.2 Cargo Build Configuration

```toml
[profile.release]
lto = true
codegen-units = 1
opt-level = "z"        # Size optimization
strip = true
panic = "abort"
```

### 7.3 Binary Structure

```rust
struct BinaryOutput {
    // Embedded static assets
    static_assets: EmbeddedAssets,
    
    // Server entry point
    server: ServerEntry,
    
    // Route handlers (generated)
    handlers: Vec<RouteHandler>,
    
    // Component registry
    components: ComponentRegistry,
    
    // Islands manifest
    islands: IslandsManifest,
    
    // Client runtime (embedded JS)
    client_runtime: EmbeddedJs,
}
```

---

## 8. Implementation Roadmap

### Phase 0: Foundation (Current)

| Task | Status | Notes |
|------|--------|-------|
| TSX Parser | ✅ Done | Recursive descent, ~57KB |
| HIR representation | ✅ Done | Full AST coverage |
| Code generator (basic) | ✅ Done | Components, hooks, JSX |
| Signal system | ✅ Done | Fine-grained reactivity |
| Hooks runtime | ✅ Done | useState, useEffect, etc. |
| Islands architecture | ✅ Done | Config, SSR, client runtime |
| Example project | ✅ Done | my-blog example |

### Phase 1: Completeness (2-3 weeks)

| Task | Priority | Deliverable |
|------|----------|-------------|
| Full route handlers | P0 | GET, POST, PUT, DELETE, PATCH |
| Middleware chain | P0 | `_middleware.ts` → Axum middleware |
| Layout system | P0 | `_layout.tsx` nesting |
| html! proc macro | P0 | Full JSX → html! transformation |
| Dev server | P1 | File watching, WebSocket HMR |
| Static assets | P1 | `static/` directory serving |

### Phase 2: Quality (3-4 weeks)

| Task | Priority | Deliverable |
|------|----------|-------------|
| Error messages | P0 | Source locations, suggestions |
| Type checking | P1 | Validate TS annotations |
| Source maps | P2 | Debug generated Rust |
| Better codegen | P0 | Idiomatic Rust output |
| Testing | P0 | Unit + integration tests |
| Documentation | P1 | Migration guide from Fresh |

### Phase 3: Performance (2-3 weeks)

| Task | Priority | Deliverable |
|------|----------|-------------|
| Incremental build | P0 | Cache + parallel transpile |
| Binary size | P1 | < 1MB target |
| Cold start | P0 | < 20ms target |
| Throughput | P1 | > 50k req/s target |

### Phase 4: Ecosystem (4+ weeks)

| Task | Priority | Deliverable |
|------|----------|-------------|
| Database ORM | P2 | SQLx integration |
| Auth helpers | P2 | Session management |
| Edge deployment | P3 | Cloudflare Workers |
| VSCode extension | P2 | Syntax highlighting |
| DevTools | P3 | Browser extension |

---

## 9. Performance Targets

### 9.1 Build Performance

| Metric | Target | Current | Method |
|--------|--------|---------|--------|
| Cold transpile | <500ms | ~800ms | `time runts build` |
| Incremental | <50ms | ~100ms | Single file change |
| Full build | <5s | ~10s | 8-core parallel |
| Memory (build) | <500MB | ~600MB | `valgrind` |

### 9.2 Runtime Performance

| Metric | Target | Comparison |
|--------|--------|------------|
| Cold start | <20ms | Deno Fresh: ~50ms |
| Throughput | >50k req/s | wrk benchmark |
| Memory (idle) | <5MB | Node.js: ~50MB |
| Memory (active) | <50MB | Node.js: ~200MB |
| Binary size | <1MB | Go: ~10MB |

### 9.3 Developer Experience

| Metric | Target | Current |
|--------|--------|---------|
| Dev server start | <500ms | ~1s |
| Hot reload (browser) | <100ms | ~200ms |
| TypeScript check | <200ms | ~500ms |
| Error display | <50ms | ~100ms |

---

## 10. Error Handling

### 10.1 Error Codes

| Code | Category | Description |
|------|----------|-------------|
| E001 | Parse | Unexpected token |
| E002 | Parse | Unclosed bracket/brace |
| E003 | Parse | Invalid JSX syntax |
| E004 | Parse | Invalid TypeScript syntax |
| E010 | Type | Unsupported type |
| E011 | Type | Type mismatch |
| E012 | Type | Cannot infer type |
| E020 | Import | Module not found |
| E021 | Import | Named export not found |
| E030 | Component | Hook called in wrong order |
| E031 | Component | Invalid hook call |
| E040 | Route | Invalid route pattern |
| E041 | Route | Missing handler export |
| E050 | Codegen | Unsupported feature |
| E051 | Codegen | Cannot transform expression |

### 10.2 Error Format

```json
{
  "error": {
    "code": "E030",
    "message": "Hooks must be called at the top level",
    "location": {
      "file": "islands/Counter.tsx",
      "line": 15,
      "column": 3
    },
    "hint": "Move the useState call outside of the if statement",
    "context": {
      "hook": "useState",
      "hookName": "count"
    }
  }
}
```

---

## Appendix A: File Structure

```
runts/
├── Cargo.toml                    # Workspace manifest
├── src/
│   ├── main.rs                   # CLI entry point
│   ├── lib.rs                    # Library exports
│   ├── config.rs                 # Config loading
│   ├── transpile/                # Transpilation pipeline
│   │   ├── mod.rs               # Pipeline orchestration
│   │   ├── parser.rs            # Recursive descent parser
│   │   ├── hir.rs               # High-level IR
│   │   ├── analyzer.rs          # Semantic analysis
│   │   ├── codegen.rs           # Rust code generation
│   │   └── tests.rs             # Transpile tests
│   ├── runtime/                  # Runtime system
│   │   ├── mod.rs              # Module exports
│   │   ├── signals.rs          # Signal implementation
│   │   ├── hooks.rs            # Hooks implementation
│   │   ├── component.rs        # Component trait
│   │   ├── vdom.rs             # Virtual DOM types
│   │   ├── islands.rs          # Islands architecture
│   │   └── prelude.rs          # Public API
│   └── commands/                # CLI commands
│       ├── mod.rs
│       ├── init.rs
│       ├── dev.rs
│       └── build.rs
├── crates/
│   ├── runts-lib/              # Runtime library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── runtime/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── prelude.rs
│   │   │   │   ├── vdom.rs
│   │   │   │   ├── signals.rs
│   │   │   │   ├── hooks.rs
│   │   │   │   └── islands.rs
│   │   │   └── macros.rs
│   │   └── build.rs
│   ├── runts-client/           # Client runtime (JS)
│   │   ├── src/
│   │   │   └── runtime.ts
│   │   └── build.rs
│   └── runts-macros/           # Proc macros
│       ├── src/
│       │   ├── lib.rs
│       │   ├── component.rs
│       │   └── html.rs
│       └── build.rs
├── examples/
│   └── my-blog/                # Example project
├── tests/
│   ├── unit/                   # Unit tests
│   └── integration/            # Integration tests
└── docs/
    ├── SPEC.md                 # Specification
    └── TECHNICAL_SPEC.md        # This document
```

---

## Appendix B: Cargo.toml Dependencies

```toml
[workspace]
members = [
    ".",
    "crates/runts-lib",
    "crates/runts-client",
    "crates/runts-macros",
]

[workspace.dependencies]
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.0", features = ["full"] }
axum = "0.7"
tower = "0.4"
tower-http = { version = "0.5", features = ["fs", "cors"] }
parking_lot = "0.12"
notify = "6.1"
```

---

**End of Technical Specification**
