# runts Transpilation Strategy

## Overview

This document details the transpilation strategy for converting Fresh/Preact TypeScript/TSX to Rust code. It covers the parsing approach, type system mapping, JSX transformation, and code generation patterns.

## 1. Parsing Approach

### 1.1 Custom Recursive Descent Parser

We use a custom recursive descent parser instead of `swc` for:

| Benefit | Description |
|---------|-------------|
| Zero dependencies | No external parsing libraries |
| Controlled scope | Only supports our subset |
| Fast parsing | Hand-tuned for common patterns |
| Clear errors | Custom error messages |

### 1.2 Parser Architecture

```
Source Text
    │
    ▼
┌─────────────────────────┐
│        Lexer            │
│  (Token Stream)         │
│  - Keywords             │
│  - Operators             │
│  - Identifiers           │
│  - Literals              │
└─────────────────────────┘
    │
    ▼
┌─────────────────────────┐
│        Parser           │
│  (Recursive Descent)    │
│  - Expression parser     │
│  - Statement parser      │
│  - Type parser           │
│  - JSX parser            │
└─────────────────────────┘
    │
    ▼
┌─────────────────────────┐
│         HIR             │
│  (High-Level IR)        │
│  - Typed AST            │
│  - Semantic info        │
└─────────────────────────┘
```

### 1.3 Token Types

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    String(String),
    Number(f64),
    Boolean(bool),
    Ident(String),
    
    // Keywords
    KwConst,
    KwLet,
    KwVar,
    KwFunction,
    KwAsync,
    KwReturn,
    KwIf,
    KwElse,
    KwFor,
    KwWhile,
    KwImport,
    KwExport,
    KwDefault,
    KwFrom,
    KwType,
    KwInterface,
    KwClass,
    
    // Operators
    OpEq,
    OpPlus,
    OpMinus,
    OpStar,
    OpSlash,
    OpEqEq,
    OpNotEq,
    OpLt,
    OpGt,
    OpAndAnd,
    OpOrOr,
    OpQuestion,
    OpColon,
    
    // JSX
    JSXOpen(String),      // <tag
    JSXClose(String),     // </tag>
    JSXSelfClose(String),// <tag/
    JSXAttr(String),      // attr=
    
    // Punctuation
    LParen, RParen,
    LBrace, RBrace,
    LBracket, RBracket,
    Comma, Dot, Semicolon,
    
    Eof,
}
```

## 2. High-Level IR (HIR)

### 2.1 Module Structure

```rust
pub struct Module {
    pub source: String,
    pub items: Vec<ModuleItem>,
    pub types: HashMap<String, TypeDef>,
}

pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}
```

### 2.2 Type System

```rust
pub enum Type {
    // Primitives
    String,
    Number,
    Boolean,
    Null,
    Undefined,
    
    // Composite
    Array { elem: Box<Type> },
    Object { members: Vec<ObjectMember> },
    Tuple { types: Vec<Type> },
    
    // Unions/Intersections
    Union { types: Vec<Type> },
    Intersection { types: Vec<Type> },
    
    // Special
    Ref { name: String, generics: Vec<Type> },
    Function { params: Vec<Type>, ret: Box<Type> },
}
```

## 3. Type Mapping

### 3.1 Primitive Types

| TypeScript | Rust | Notes |
|-----------|------|-------|
| `string` | `String` | UTF-8 owned string |
| `number` | `f64` | IEEE 754 double |
| `boolean` | `bool` | Native boolean |
| `null` | `()` | Unit type for void |
| `undefined` | `()` | Unit type |
| `void` | `()` | Unit type |
| `never` | `!` | Never type (unreachable) |
| `any` | `serde_json::Value` | JSON value |
| `unknown` | `serde_json::Value` | JSON value |

### 3.2 Compound Types

| TypeScript | Rust | Notes |
|-----------|------|-------|
| `T[]` | `Vec<T>` | Dynamic array |
| `Array<T>` | `Vec<T>` | Same as above |
| `[A, B, C]` | `(A, B, C)` | Tuple |
| `{ a: T; b: U }` | `struct { pub a: T; pub b: U }` | Struct |

### 3.3 Union Types

```typescript
// T | null → Option<T>
type MaybeString = string | null;  // Option<String>

// A | B → enum
type Result = Success | Error;   // enum Result { Success, Error }
```

### 3.4 Interface to Struct

```typescript
// TypeScript
interface User {
    id: number;
    name: string;
    email?: string;
}

// Generated Rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: f64,
    pub name: String,
    pub email: Option<String>,
}
```

## 4. JSX Transformation

### 4.1 Basic Elements

```tsx
// Input
<div className="container">Hello</div>

// Output
html!(<div class_name="container">{"Hello"}</div>)
```

### 4.2 Components

```tsx
// Input
<Counter initial={0} step={1} />

// Output
counter(initial: f64, step: f64)
```

### 4.3 Event Handlers

| JSX | Rust (html! macro) |
|-----|---------------------|
| `onClick={fn}` | `on_click={fn}` |
| `onChange={fn}` | `on_change={fn}` |
| `onSubmit={fn}` | `on_submit={fn}` |
| `onInput={fn}` | `on_input={fn}` |
| `onFocus={fn}` | `on_focus={fn}` |
| `onBlur={fn}` | `on_blur={fn}` |
| `onKeyDown={fn}` | `on_key_down={fn}` |

### 4.4 Conditional Rendering

```tsx
// Input
{show && <Modal />}

// Output
{ show.then_some(html!(<Modal />)) }
```

### 4.5 List Rendering

```tsx
// Input
{items.map(item => (
    <li key={item.id}>{item.text}</li>
))}

// Output
html! {
    { for items.iter().map(|item| html!(
        <li key={item.id.clone()}>{ item.text.clone() }</li>
    )) }
}
```

### 4.6 Fragments

```tsx
// Input
<>
    <Header />
    <Content />
</>

// Output
html! {
    <>
        <Header />
        <Content />
    </>
}
```

## 5. Hook Transformations

### 5.1 useState

```tsx
// Input
const [count, setCount] = useState(0);
setCount(count + 1);

// Output
let (count, set_count) = use_state(|| 0.0);
set_count(count + 1.0);
```

### 5.2 useEffect

```tsx
// Input
useEffect(() => {
    console.log(count);
    return () => cleanup();
}, [count]);

// Output
use_effect(|| {
    println!("{:?}", count);
    Some(|| cleanup())
}, [count]);
```

### 5.3 useRef

```tsx
// Input
const inputRef = useRef<HTMLInputElement>(null);
inputRef.current?.focus();

// Output
let input_ref = use_ref(|| None::<web_sys::HtmlInputElement>);
if let Some(el) = input_ref.get() {
    el.focus();
}
```

### 5.4 useMemo

```tsx
// Input
const expensive = useMemo(() => computeExpensive(), [dep]);

// Output
let expensive = use_memo(|| compute_expensive(), [dep]);
```

## 6. Signal Transformations

### 6.1 Creating Signals

```tsx
// Input
import { signal } from "@preact/signals";
const count = signal(0);

// Output
use runts_lib::runtime::signals::*;
let count = signal(0);
```

### 6.2 Reading/Writing

```tsx
// Input
count.value
count.value++

// Output
count.get()
count.set(count.get() + 1)
```

### 6.3 Computed Signals

```tsx
// Input
const doubled = computed(() => count.value * 2);

// Output
let doubled = computed(|| count.get() * 2);
```

## 7. Pattern Transformations

### 7.1 Destructuring

```tsx
// Object destructuring
const { name, age } = user;
// → let name = user.name; let age = user.age;

// Array destructuring
const [first, ...rest] = items;
// → let first = items[0]; let rest = &items[1..];
```

### 7.2 Arrow Functions

```tsx
// Input
const add = (a, b) => a + b;
const greet = name => `Hello, ${name}!`;
const log = () => console.log("hello");

// Output
let add = |a, b| a + b;
let greet = |name| format!("Hello, {}!", name);
let log = || println!("hello");
```

### 7.3 Template Literals

```tsx
// Input
`Hello, ${name}!`
`Count: ${count + 1}`

// Output
format!("Hello, {}!", name)
format!("Count: {}", count + 1.0)
```

## 8. Function Transformations

### 8.1 Async Functions

```tsx
// Input
async function fetchData(url: string): Promise<Data> {
    const res = await fetch(url);
    return res.json();
}

// Output
async fn fetch_data(url: String) -> Result<Data, Error> {
    let res = reqwest::get(&url).await?;
    Ok(res.json().await?)
}
```

### 8.2 Default Parameters

```tsx
// Input
function greet(name = "World") {
    return `Hello, ${name}!`;
}

// Output
fn greet(name: String) -> String {
    let name = if name.is_empty() { "World".to_string() } else { name };
    format!("Hello, {}!", name)
}
```

## 9. Code Generation Patterns

### 9.1 Component Detection

```rust
// Components are functions starting with uppercase
fn is_component(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

// Components get the #[component] attribute
if is_component(&func.name) {
    output.push_str("#[component]\n");
}
```

### 9.2 Imports Generation

```rust
// Standard imports for all generated modules
const DEFAULT_IMPORTS: &str = r#"
use runts_lib::runtime::prelude::*;
use serde::{Serialize, Deserialize};
"#;
```

### 9.3 Type Derives

```rust
// Interfaces get Serialize/Deserialize
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Props {
    pub name: String,
    pub count: f64,
}
```

## 10. Error Handling

### 10.1 Parse Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Unexpected token at position {pos}: expected {expected}, found {found}")]
    UnexpectedToken { pos: usize, expected: String, found: String },
    
    #[error("Unterminated string at position {pos}")]
    UnterminatedString { pos: usize },
    
    #[error("Invalid JSX at position {pos}: {message}")]
    InvalidJSX { pos: usize, message: String },
}
```

### 10.2 Type Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("Type mismatch: expected {expected}, found {found}")]
    Mismatch { expected: String, found: String },
    
    #[error("Unknown type: {0}")]
    UnknownType(String),
    
    #[error("Unsupported type operation: {0}")]
    UnsupportedOp(String),
}
```

## 11. Optimization Strategies

### 11.1 Constant Folding

```tsx
// Input
const x = 1 + 2;

// Output
let x = 3.0;
```

### 11.2 Dead Code Elimination

```tsx
// Input (IS_BROWSER is false on server)
if (IS_BROWSER) {
    // This is eliminated in SSR
}

// Output (empty in SSR build)
```

### 11.3 Inlining Small Components

Small components are inlined to reduce function call overhead.

---

*Last Updated: 2026-05-26*
