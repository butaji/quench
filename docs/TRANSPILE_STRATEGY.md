# runts Transpilation Strategy

> **⚠️ STALE DOCUMENT:** This doc describes the pre-rquickjs architecture where dev mode used a HIR interpreter. Several sections (especially §8 and §12.6/12.7) still reference the interpreter. The current dev engine is **rquickjs** with **Yoga** layout. The compile path described here remains accurate. Update in progress — see `tasks/031-update-docs.md`.
>
> How TypeScript/TSX becomes native Rust — from source text to binary.

---

## 1. Pipeline Overview

```
TS/TSX Source
    │
    ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Lexer     │────▶│   Parser    │────▶│    HIR      │────▶│  Analyzer   │
│             │     │ (Recursive  │     │ (Normalized │     │ (Semantic   │
│             │     │  Descent)   │     │   Typed IR) │     │  Analysis)  │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
                                                                  │
                    ┌─────────────────────────────────────────────┘
                    ▼
            ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
            │  Transform  │────▶│   Codegen   │────▶│   Rust AST  │
            │ (Lowering)  │     │  (String)   │     │  (syn crate)│
            └─────────────┘     └─────────────┘     └─────────────┘
                                                          │
                    ┌─────────────────────────────────────┘
                    ▼
            ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
            │  cargo check│────▶│cargo build  │────▶│Native Binary│
            │             │     │--release    │     │             │
            └─────────────┘     └─────────────┘     └─────────────┘
```

**Two execution paths:**
- **Development**: Source → Parser → HIR → **Interpreter** (no codegen)
- **Production**: Source → Parser → HIR → Analyzer → **Codegen** → Rust → cargo build

---

## 2. Lexical Analysis

Hand-written lexer with zero dependencies. ~800 lines.

### Token types:

```rust
pub enum TokenKind {
    // Literals
    Number(f64),
    String(String),
    Template(String),
    True, False, Null, Undefined,
    
    // Identifiers
    Ident(String),
    
    // Keywords
    Let, Const, Var, Function, Return, If, Else,
    For, While, Break, Continue, Switch, Case, Default,
    Try, Catch, Throw, Finally, Async, Await,
    Import, Export, From, As, Default,
    Class, Extends, New, This, Super,
    Interface, Type, Enum,
    
    // Punctuation
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Semi, Comma, Dot, Colon, Question, Arrow,
    
    // Operators
    Plus, Minus, Star, Slash, Percent,
    Eq, EqEq, EqEqEq, NotEq, NotEqEq,
    Lt, LtEq, Gt, GtEq,
    AndAnd, OrOr, Bang,
    PlusEq, MinusEq, StarEq, SlashEq,
    
    // JSX
    JSXText(String),
    JSXExprStart, JSXExprEnd,
    JSXTagStart, JSXTagEnd,
    JSXClosingTagStart,
    JSXFragmentStart, JSXFragmentEnd,
    
    // Types
    Colon, <, >, <=, >=,
    Pipe, &, <<, >>,
}
```

### Key features:

- **No regex-based tokenization**: Character-by-character for speed and control
- **Template literal scanning**: Handles nested `${}` expressions
- **JSX mode toggle**: Lexer switches between JS and JSX contexts
- **Type annotation mode**: Distinguishes `x: number` from `x ? a : b`

### Performance:

- ~50MB/s tokenization (single-threaded)
- No allocations for single-character tokens
- String interning for common identifiers (`div`, `span`, `className`)

---

## 3. Parsing

Recursive descent parser with Pratt parsing for expressions.

### Entry points:

```rust
impl Parser {
    pub fn parse_module(&mut self) -> Result<Module, ParseError> {
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        let mut declarations = Vec::new();
        
        while !self.is_eof() {
            self.skip_whitespace_and_comments();
            
            let stmt = if self.peek_import() {
                self.parse_import()
            } else if self.peek_export() {
                self.parse_export()
            } else {
                self.parse_statement()
            }?;
            
            declarations.push(stmt);
        }
        
        Ok(Module { imports, exports, declarations, ... })
    }
}
```

### Expression precedence (Pratt parser):

| Precedence | Operators | Example |
|-----------|-----------|---------|
| 1 | `=` `+=` `-=` `*=` `/=` | Assignment |
| 2 | `? :` | Ternary |
| 3 | `\|\|` | Logical OR |
| 4 | `&&` | Logical AND |
| 5 | `==` `!=` `===` `!==` | Equality |
| 6 | `<` `>` `<=` `>=` | Comparison |
| 7 | `+` `-` | Additive |
| 8 | `*` `/` `%` | Multiplicative |
| 9 | `!` `-` `++` `--` | Unary |
| 10 | `.` `[]` `()` `?.` | Member / Call |

### JSX parsing:

```rust
fn parse_jsx_element(&mut self) -> Result<JSXElement, ParseError> {
    // <TagName attributes> children </TagName>
    self.expect(JSXTagStart)?;
    let name = self.parse_jsx_name()?;
    let attrs = self.parse_jsx_attributes()?;
    
    if self.peek(JSXTagEndSelfClosing) {
        return Ok(JSXElement::self_closing(name, attrs));
    }
    
    self.expect(JSXTagEnd)?;
    let children = self.parse_jsx_children()?;
    self.expect(JSXClosingTagStart)?;
    self.expect_jsx_name(&name)?;
    self.expect(JSXTagEnd)?;
    
    Ok(JSXElement::with_children(name, attrs, children))
}
```

### Type annotation parsing:

```typescript
// TypeScript
function f(x: string | number, y?: boolean): void {}

// Parsed HIR
FunctionDecl {
    name: "f",
    params: [
        Param { name: "x", type: Union([String, Number]) },
        Param { name: "y", type: Optional(Bool) },
    ],
    return_type: Void,
    body: [...],
}
```

### Recovery strategy:

On parse error, the parser:
1. Reports the error with line/column
2. Attempts to sync to the next statement boundary (`;`, `}`, or newline at top level)
3. Continues parsing to find additional errors

---

## 4. HIR (High-level IR)

HIR is the universal representation used by both the interpreter and code generator.

### Design goals:

1. **Normalization**: All TS sugar desugars to core constructs
2. **Type preservation**: Type annotations are preserved for codegen
3. **Fresh semantics**: JSX, hooks, and islands have first-class representations

### HIR node types:

```rust
/// Expression — covers all value-producing constructs
pub enum Expr {
    /// Literals: 42, "hello", true, null, undefined
    Literal(Literal),
    
    /// Variable reference: x
    Ident(String),
    
    /// Binary operation: a + b, a === b
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    
    /// Unary operation: !x, -x, typeof x
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    
    /// Function call: f(x, y)
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        type_args: Vec<TypeAnn>,
    },
    
    /// Member access: obj.prop, obj[method]
    Member {
        obj: Box<Expr>,
        prop: MemberProp,
    },
    
    /// Arrow function: (x) => x * 2
    ArrowFn {
        params: Vec<Param>,
        return_type: Option<TypeAnn>,
        body: FnBody,
    },
    
    /// Regular function expression
    FnExpr {
        name: Option<String>,
        params: Vec<Param>,
        return_type: Option<TypeAnn>,
        body: Block,
        is_async: bool,
    },
    
    /// Array literal: [1, 2, 3]
    Array(Vec<Expr>),
    
    /// Object literal: {a: 1, b: 2}
    Object(Vec<ObjectProp>),
    
    /// JSX element: <div class="x">{children}</div>
    JSXElement(JSXElement),
    
    /// JSX fragment: <>...</>
    JSXFragment(Vec<Expr>),
    
    /// Template literal: `hello ${name}`
    Template(Vec<TemplatePart>),
    
    /// Await expression
    Await(Box<Expr>),
    
    /// Conditional: a ? b : c
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    
    /// Array spread in object: {...obj, a: 1}
    Spread(Box<Expr>),
    
    /// Type assertion: expr as Type
    TypeAssertion(Box<Expr>, TypeAnn),
    
    /// Update expression: i++, --j
    Update {
        op: UpdateOp,
        expr: Box<Expr>,
        prefix: bool,
    },
    
    /// Optional chaining: obj?.prop?.method?.()
    OptionalChain(Box<Expr>),
    
    /// Nullish coalescing: a ?? b
    NullishCoalesce {
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

/// Statement — covers all effect-producing constructs
pub enum Stmt {
    /// Variable declaration: let x = 5
    Let {
        name: String,
        init: Expr,
        type_ann: Option<TypeAnn>,
        mutable: bool, // inferred from usage
    },
    
    /// Expression statement: console.log("x")
    Expr(Expr),
    
    /// Return statement
    Return(Option<Expr>),
    
    /// Block: { ... }
    Block(Block),
    
    /// If statement
    If {
        cond: Expr,
        then: Block,
        else_: Option<Block>,
    },
    
    /// For loop: for (init; cond; update) { body }
    For {
        init: Option<Expr>,
        cond: Option<Expr>,
        update: Option<Expr>,
        body: Block,
    },
    
    /// For...of loop: for (const x of xs) { body }
    ForOf {
        var: String,
        iterable: Expr,
        body: Block,
    },
    
    /// While loop
    While {
        cond: Expr,
        body: Block,
    },
    
    /// Try/catch/finally
    Try {
        body: Block,
        catch: Option<CatchClause>,
        finally: Option<Block>,
    },
    
    /// Break
    Break,
    
    /// Continue
    Continue,
    
    /// Switch statement
    Switch {
        expr: Expr,
        cases: Vec<SwitchCase>,
    },
}
```

### JSX HIR:

```rust
pub struct JSXElement {
    pub name: JSXName,
    pub attrs: Vec<JSXAttr>,
    pub children: Vec<JSXChild>,
    pub is_self_closing: bool,
}

pub enum JSXName {
    Ident(String),      // div, Counter
    Member(String, String), // MyComponents.Button
    Fragment,
}

pub enum JSXAttr {
    /// Regular attribute: class="foo"
    Named { name: String, value: JSXAttrValue },
    /// Spread attribute: {...props}
    Spread(Expr),
    /// Boolean attribute: disabled
    Boolean(String),
}

pub enum JSXAttrValue {
    String(String),
    Expr(Expr),
}

pub enum JSXChild {
    Text(String),
    Expr(Expr),
    Element(JSXElement),
    Fragment(Vec<JSXChild>),
}
```

### Normalization examples:

```typescript
// TypeScript
const doubled = items.map(x => x * 2).filter(x => x > 5);

// HIR
Let {
    name: "doubled",
    init: Call {
        callee: Member {
            obj: Call {
                callee: Member { obj: Ident("items"), prop: "map" },
                args: [ArrowFn { params: ["x"], body: Expr(Binary { op: Mul, left: Ident("x"), right: Literal(Number(2.0)) }) }],
            },
            prop: "filter",
        },
        args: [ArrowFn { params: ["x"], body: Expr(Binary { op: Gt, left: Ident("x"), right: Literal(Number(5.0)) }) }],
    },
}
```

---

## 5. Semantic Analysis

### 5.1 Route Detection

```rust
pub fn extract_route_info(module: &Module, file_path: &Path) -> Option<RouteInfo> {
    let pattern = file_path_to_route_pattern(file_path);
    let mut handlers = Vec::new();
    let mut component = None;
    
    for decl in &module.declarations {
        match decl {
            // export const handler = { GET: fn, POST: fn }
            Decl::Export(Export::Named { name: "handler", value }) => {
                if let Expr::Object(props) = value {
                    for prop in props {
                        if let PropKey::Ident(method) = &prop.key {
                            handlers.push(RouteMethod::from_str(method));
                        }
                    }
                }
            }
            
            // export default function Page() {}
            Decl::Export(Export::Default(expr)) => {
                component = Some(extract_component_name(expr));
            }
            
            _ => {}
        }
    }
    
    Some(RouteInfo { pattern, handlers, component })
}
```

### 5.2 Island Detection

```rust
pub fn extract_island_info(module: &Module, file_path: &Path) -> Option<IslandInfo> {
    if !is_in_islands_dir(file_path) {
        return None;
    }
    
    // Find the default export component
    let component = module.exports.iter()
        .find_map(|e| match e {
            Export::Default(expr) => Some(expr),
            _ => None,
        })?;
    
    // Extract props interface
    let props_type = extract_props_type(component);
    
    // Detect hydration mode from directive comments
    let hydration = detect_hydration_mode(module);
    
    Some(IslandInfo {
        name: component_name(component),
        props_type,
        hydration,
        file_path: file_path.to_path_buf(),
    })
}
```

### 5.3 Hook Validation

```rust
pub fn validate_hooks(module: &Module) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    
    for func in module.all_functions() {
        let mut hook_calls = Vec::new();
        let mut conditional_depth = 0;
        let mut loop_depth = 0;
        
        visit_expr(func.body, |expr| {
            match expr {
                Expr::Call { callee: Expr::Ident(name), .. } if is_hook(name) => {
                    if conditional_depth > 0 {
                        errors.push(Diagnostic::hook_in_conditional(name, expr.span()));
                    }
                    if loop_depth > 0 {
                        errors.push(Diagnostic::hook_in_loop(name, expr.span()));
                    }
                    hook_calls.push(name.clone());
                }
                Expr::If { .. } => conditional_depth += 1,
                Expr::While { .. } | Expr::For { .. } => loop_depth += 1,
                _ => {}
            }
        });
        
        // Check hook call order consistency
        if hook_calls != deduplicate_consecutive(hook_calls) {
            errors.push(Diagnostic::inconsistent_hook_order(func.span()));
        }
    }
    
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}
```

### 5.4 Type Extraction

Interfaces are extracted for:
- Props types (component props → struct fields)
- Handler data types (route data → response types)
- Route params (URL params → handler arguments)
- Middleware state (shared state → HashMap types)

```rust
pub fn extract_interface_types(module: &Module) -> Vec<InterfaceType> {
    module.declarations.iter()
        .filter_map(|d| match d {
            Decl::Interface(name, members) => {
                Some(InterfaceType {
                    name: name.clone(),
                    fields: members.iter().map(|m| Field {
                        name: m.name.clone(),
                        type: m.type_ann.clone(),
                        optional: m.optional,
                    }).collect(),
                })
            }
            Decl::TypeAlias(name, type_ann) => {
                Some(expand_type_alias(name, type_ann))
            }
            _ => None,
        })
        .collect()
}
```

---

## 6. Code Generation

### 6.1 Module-level generation

```rust
pub fn generate_module(&mut self,
    module: &Module,
    is_route: bool,
    is_island: bool,
) -> Result<String, GenError> {
    let mut output = String::new();
    
    // Header
    output.push_str("//! Generated by runts\n\n");
    output.push_str("use runts_lib::runtime::prelude::*;\n");
    output.push_str("use serde::{Serialize, Deserialize};\n");
    output.push_str("use axum::{response::IntoResponse, body::Body};\n\n");
    
    // Extracted types (interfaces → structs)
    for iface in &module.interfaces {
        output.push_str(&self.generate_interface(iface));
        output.push('\n');
    }
    
    // Imports (translated to Rust use statements)
    for import in &module.imports {
        output.push_str(&self.generate_import(import));
    }
    output.push('\n');
    
    // Handler (for routes)
    if is_route && module.has_handler_export() {
        output.push_str(&self.generate_handler(module)?);
        output.push('\n');
    }
    
    // Component (default export or named export)
    if module.has_component_export() {
        output.push_str(&self.generate_component(module, is_island)?);
    }
    
    Ok(output)
}
```

### 6.2 Component generation

```rust
fn generate_component(
    &mut self,
    module: &Module,
    is_island: bool,
) -> Result<String, GenError> {
    let component = module.default_export()
        .ok_or(GenError::NoComponent)?;
    
    let name = component.name.clone()
        .unwrap_or_else(|| "DefaultComponent".to_string());
    
    let props_type = component.props_type
        .clone()
        .unwrap_or_else(|| "()".to_string());
    
    let mut output = String::new();
    
    // Props struct
    if let Some(iface) = module.find_interface(&props_type) {
        output.push_str(&self.generate_props_struct(iface));
    }
    
    // Component function
    let attrs = if is_island {
        "#[component]\n#[island]"
    } else {
        "#[component]"
    };
    
    output.push_str(&format!(
        "{}\npub fn {}(props: {}) -> VNode {{\n",
        attrs, to_snake_case(&name), props_type
    ));
    
    self.indent += 1;
    
    // Body
    for stmt in &component.body {
        output.push_str(&self.generate_stmt(stmt));
    }
    
    self.indent -= 1;
    output.push_str("}\n");
    
    Ok(output)
}
```

### 6.3 JSX → html! macro

```rust
fn generate_jsx(&mut self,
    element: &JSXElement,
) -> String {
    let mut output = String::new();
    
    match &element.name {
        JSXName::Fragment => {
            output.push_str("html!(\u003c>");
            for child in &element.children {
                output.push_str(&self.generate_jsx_child(child));
            }
            output.push_str("</>)");
        }
        
        JSXName::Ident(tag) if is_html_tag(tag) => {
            output.push_str(&format!("html!(\u003c{}", tag));
            
            for attr in &element.attrs {
                output.push_str(&self.generate_jsx_attr(attr));
            }
            
            if element.children.is_empty() && element.is_self_closing {
                output.push_str(" />)");
            } else {
                output.push_str(">");
                for child in &element.children {
                    output.push_str(&self.generate_jsx_child(child));
                }
                output.push_str(&format!("</{}\u003e)", tag));
            }
        }
        
        JSXName::Ident(component) |
        JSXName::Member(_, component) => {
            // Component invocation: <Counter initial={5} />
            output.push_str(&format!("{}(", to_snake_case(component)));
            
            // Props struct literal
            let mut props_fields = Vec::new();
            for attr in &element.attrs {
                if let JSXAttr::Named { name, value } = attr {
                    let rust_name = to_snake_case(name);
                    let rust_value = self.generate_jsx_attr_value(value);
                    props_fields.push(format!("{}: {}", rust_name, rust_value));
                }
            }
            
            output.push_str("Props { ");
            output.push_str(&props_fields.join(", "));
            output.push_str(" })");
        }
    }
    
    output
}
```

### 6.4 Array method translation

```rust
fn generate_array_method(
    &mut self,
    obj: &Expr,
    method: &str,
    args: &[Expr],
) -> String {
    let obj_code = self.generate_expr(obj);
    
    match method {
        "map" => {
            let closure = self.generate_expr(&args[0]);
            format!(
                "{}.iter().map({}).collect::<Vec<_>>()",
                obj_code, closure
            )
        }
        
        "filter" => {
            let closure = self.generate_expr(&args[0]);
            format!(
                "{}.iter().filter({}).cloned().collect::<Vec<_>>()",
                obj_code, closure
            )
        }
        
        "find" => {
            let closure = self.generate_expr(&args[0]);
            format!("{}.iter().find({}).cloned()", obj_code, closure)
        }
        
        "reduce" => {
            let init = self.generate_expr(&args[0]);
            let closure = if args.len() > 1 {
                self.generate_expr(&args[1])
            } else {
                self.generate_expr(&args[0])
            };
            format!(
                "{}.iter().fold({}, {})",
                obj_code, init, closure
            )
        }
        
        "includes" => {
            let item = self.generate_expr(&args[0]);
            format!("{}.contains(&{})", obj_code, item)
        }
        
        "sort" => {
            if args.is_empty() {
                format!("{{ let mut __tmp = {}; __tmp.sort(); __tmp }}", obj_code)
            } else {
                let cmp = self.generate_expr(&args[0]);
                format!(
                    "{{ let mut __tmp = {}; __tmp.sort_by({}); __tmp }}",
                    obj_code, cmp
                )
            }
        }
        
        "join" => {
            let sep = self.generate_expr(&args[0]);
            format!("{}.join(&{})", obj_code, sep)
        }
        
        "push" => {
            let item = self.generate_expr(&args[0]);
            format!("{{ let mut __tmp = {}; __tmp.push({}); __tmp }}", obj_code, item)
        }
        
        "length" => {
            format!("{}.len() as f64", obj_code)
        }
        
        _ => {
            // Unknown method — generate a comment and todo
            format!(
                "{{ /* TODO: array method '{}' */ {}.{}",
                method, obj_code, method
            )
        }
    }
}
```

### 6.5 String method translation

```rust
fn generate_string_method(
    &mut self,
    obj: &Expr,
    method: &str,
    args: &[Expr],
) -> String {
    let obj_code = self.generate_expr(obj);
    
    match method {
        "split" => {
            let sep = self.generate_expr(&args[0]);
            format!("{}.split(&{}).collect::<Vec<_>>()", obj_code, sep)
        }
        "trim" => format!("{}.trim()", obj_code),
        "startsWith" => {
            let prefix = self.generate_expr(&args[0]);
            format!("{}.starts_with(&{})", obj_code, prefix)
        }
        "endsWith" => {
            let suffix = self.generate_expr(&args[0]);
            format!("{}.ends_with(&{})", obj_code, suffix)
        }
        "replace" => {
            let from = self.generate_expr(&args[0]);
            let to = self.generate_expr(&args[1]);
            format!("{}.replace(&{}, &{})", obj_code, from, to)
        }
        "slice" => {
            let start = self.generate_expr(&args[0]);
            if args.len() > 1 {
                let end = self.generate_expr(&args[1]);
                format!("{}[{}..{}]", obj_code, start, end)
            } else {
                format!("{}[{}..]", obj_code, start)
            }
        }
        "toLowerCase" => format!("{}.to_lowercase()", obj_code),
        "toUpperCase" => format!("{}.to_uppercase()", obj_code),
        "includes" => {
            let substr = self.generate_expr(&args[0]);
            format!("{}.contains(&{})", obj_code, substr)
        }
        "length" => format!("{}.len() as f64", obj_code),
        _ => format!("{}.{}() /* TODO: string method */", obj_code, method),
    }
}
```

---

## 7. Client JS Generation

Islands are compiled to a minimal vanilla JS runtime — no Preact, no React, no VDOM.

### 7.1 Signal runtime (JS)

```javascript
// ~2KB gzipped
class Signal {
  constructor(value) {
    this._value = value;
    this._subs = new Set();
    this._version = 0;
  }
  get value() {
    if (currentEffect) this._subs.add(currentEffect);
    return this._value;
  }
  set value(v) {
    if (this._value !== v) {
      this._value = v;
      this._version++;
      this._subs.forEach(fn => fn());
    }
  }
}

let currentEffect = null;

function effect(fn) {
  currentEffect = fn;
  fn();
  currentEffect = null;
}

function computed(fn) {
  const s = new Signal();
  effect(() => s.value = fn());
  return s;
}
```

### 7.2 Hook runtime (JS)

```javascript
// ~1KB gzipped
const hooks = [];
let hookIndex = 0;

function useState(initial) {
  const idx = hookIndex++;
  if (hooks[idx] === undefined) hooks[idx] = initial;
  const set = (v) => {
    hooks[idx] = v;
    rerender();
  };
  return [hooks[idx], set];
}

function useEffect(fn, deps) {
  const idx = hookIndex++;
  const prev = hooks[idx];
  if (!prev || !depsEqual(prev.deps, deps)) {
    if (prev && prev.cleanup) prev.cleanup();
    hooks[idx] = { deps, cleanup: fn() };
  }
}

function useMemo(fn, deps) {
  const idx = hookIndex++;
  const prev = hooks[idx];
  if (!prev || !depsEqual(prev.deps, deps)) {
    hooks[idx] = { deps, value: fn() };
  }
  return hooks[idx].value;
}
```

### 7.3 DOM binding

```javascript
// Direct DOM manipulation — no VDOM
function bind(el, signal) {
  effect(() => {
    if (el.textContent !== String(signal.value)) {
      el.textContent = signal.value;
    }
  });
}

function bindAttr(el, attr, signal) {
  effect(() => {
    el.setAttribute(attr, signal.value);
  });
}

function bindEvent(el, event, handler) {
  el.addEventListener(event, handler);
}
```

### 7.4 Island component JS output example

**Source:**
```tsx
export default function Counter({ initial }: CounterProps) {
  const [count, setCount] = useState(initial);
  
  return (
    <div>
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

**Generated JS (~800 bytes minified):**
```javascript
function Counter(props) {
  const [count, setCount] = useState(props.initial);
  
  const el = document.createElement('div');
  const p = document.createElement('p');
  bind(p, { get value() { return 'Count: ' + count.value; } });
  el.appendChild(p);
  
  const btn = document.createElement('button');
  btn.textContent = '+';
  bindEvent(btn, 'click', () => setCount(count.value + 1));
  el.appendChild(btn);
  
  return el;
}
```

---

## 8. Dev Mode vs Production Mode

### Dev Mode (rquickjs — HIR Interpreter REMOVED)

```
Request ──▶ Axum ──▶ Route match ──▶ Load HIR ──▶ Eval component
                                              │
                                              ▼
                                         ┌─────────┐
                                         │Renderer │
                                         │(to HTML)│
                                         └────┬────┘
                                              │
                                         ┌────▼────┐
                                         │ Inject  │
                                         │ islands │
                                         │ markers │
                                         └────┬────┘
                                              │
                                         ┌────▼────┐
                                         │ Response│
                                         │ (HTML)  │
                                         └─────────┘
```

**No compilation step.** HIR is parsed once and cached. File changes trigger re-parse of only the affected module.

### Production Mode (Native Binary)

```
Request ──▶ Axum ──▶ Route match ──▶ Native handler ──▶ Call component fn
                                                          │
                                                          ▼
                                                     ┌─────────┐
                                                     │ VNode   │
                                                     │render   │
                                                     └────┬────┘
                                                          │
                                                     ┌────▼────┐
                                                     │ HTML    │
                                                     │ output  │
                                                     └────┬────┘
                                                          │
                                                     ┌────▼────┐
                                                     │ Islands │
                                                     │ markers │
                                                     └─────────┘
```

**Zero interpretation overhead.** All components are native Rust functions. VNode rendering is inlinable and LTO-optimized.

---

## 9. Source Maps

Source map generation is **deferred** to the Rust compiler. We emit `#[track_caller]` attributes on generated functions and preserve original line numbers in comments:

```rust
// Original: routes/index.tsx:15
#[track_caller]
pub fn home_page(props: HomeProps) -> VNode {
    // Line 15 in original
    let greeting = "Welcome to runts!";
    // ...
}
```

For development mode, the interpreter maintains a mapping from HIR nodes to source spans and includes source locations in error stack traces.

---

## 10. Testing Strategy

### 10.1 Parser tests

```rust
#[test]
fn test_jsx_parsing() {
    let source = r#"<div className="foo" onClick={handleClick}>
  {children}
</div>"#;
    let mut parser = Parser::new();
    let module = parser.parse_source(source).unwrap();
    
    let jsx = extract_jsx(&module);
    assert_eq!(jsx.name, "div");
    assert_eq!(jsx.attrs.len(), 2);
}
```

### 10.2 Codegen tests

```rust
#[test]
fn test_array_methods() {
    let source = "const doubled = items.map(x => x * 2);";
    let rust = transpile(source).unwrap();
    
    assert!(rust.contains("iter().map"));
    assert!(rust.contains("collect::<Vec<_>>()"));
}
```

### 10.3 Round-trip tests

```rust
#[test]
fn test_roundtrip() {
    let ts = r#"export default function Page() {
  return <h1>Hello</h1>;
}"#;
    
    let rust = transpile(ts).unwrap();
    
    // Compile the generated Rust
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("lib.rs"), rust).unwrap();
    
    let output = Command::new("rustc")
        .arg(tmp.path().join("lib.rs"))
        .arg("--crate-type=lib")
        .output()
        .unwrap();
    
    assert!(output.status.success(), "Generated Rust failed to compile");
}
```

### 10.4 Fresh compatibility tests

A suite of real Fresh projects are transpiled and verified:
1. Parse without errors
2. Generate Rust that compiles
3. Produce identical HTML output for the same props

---

## 11. Performance Characteristics

| Phase | Time (1K LOC project) | Memory |
|-------|----------------------|--------|
| Lexing | ~5ms | ~1MB |
| Parsing | ~20ms | ~5MB |
| Analysis | ~15ms | ~3MB |
| Codegen | ~30ms | ~2MB |
| **Total transpile** | **~70ms** | **~11MB** |
| cargo check | ~2s | ~200MB |
| cargo build --release | ~30s | ~2GB |
| **Dev mode reload** | **~100ms** | **~50MB** |

---

## 12. Extensibility

### Adding a new language feature:

1. **Lexer**: Add token kind (if needed)
2. **Parser**: Add parse function for the construct
3. **HIR**: Add AST node variant
4. **Analyzer**: Add validation rules
5. **Codegen**: Add Rust generation
6. **Interpreter**: Add evaluation logic (if needed for dev mode)
7. **Tests**: Add parser + codegen + round-trip tests

### Adding a new standard library method:

1. **Codegen**: Add case in `generate_array_method` or `generate_string_method`
2. **Interpreter**: Add case in `eval_call`
3. **Tests**: Add test case

### Adding a new hook:

1. **Runtime**: Implement in `hooks.rs`
2. **Analyzer**: Add to hook validation list
3. **Codegen**: Add to prelude imports
4. **Client JS**: Add to client runtime
5. **Tests**: Add behavior tests
