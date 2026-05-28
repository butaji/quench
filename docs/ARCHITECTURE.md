# Runts Architecture: AST → HIR → Rust / Runtime

## Pipeline

```
TS/TSX Source (.ts, .tsx)
    │
    ▼
┌─────────────────┐
│  oxc_parser     │  ← battle-tested, handles all TS/TSX syntax
│  + oxc_ast      │
└─────────────────┘
    │
    ▼ Program<'a> (oxc AST)
┌─────────────────┐
│  HIR Builder    │  ← oxc_ast::Visit traversal, semantic analysis
│  (one pass)     │    extracts: routes, islands, hooks, types
└─────────────────┘
    │
    ▼ HIR::Module
    │
    ├───────────────┬───────────────┐
    │               │               │
    ▼               ▼               ▼
┌─────────┐   ┌─────────┐   ┌─────────┐
│ Build   │   │ Dev     │   │ Cache   │
│ Codegen │   │ Interp  │   │ (HIR)   │
└─────────┘   └─────────┘   └─────────┘
    │               │               │
    ▼               ▼               ▼
 .rs files      HTTP server    .runts/cache/
 .runts/build/  + hot-reload   (json/bincode)
    │
    ▼
 cargo build
    │
    ▼
 native binary
```

## Design Principles

1. **HIR is the contract**: Both codegen and interpreter consume the same HIR. No AST leaking into codegen/interpreter.
2. **Single-pass HIR builder**: oxc AST → HIR in one visit traversal. Semantic info (route/island/hook detection) collected during traversal.
3. **Serializable HIR**: HIR modules can be cached to disk (JSON/bincode) for incremental builds.
4. **Dev = interpret HIR, Build = codegen HIR**: Same source of truth, different consumers.
5. **Type erasure in HIR**: TypeScript types are preserved in HIR for codegen but are semantically erased at runtime (dev interpreter ignores them).

## HIR Design

### Module-level

```rust
pub struct Module {
    pub source: String,           // file path
    pub items: Vec<ModuleItem>,   // imports, exports, declarations
    pub types: Vec<TypeDecl>,     // type aliases, interfaces
    pub semantic: SemanticInfo,   // pre-computed: is_route, is_island, hooks, etc.
}

pub enum ModuleItem {
    Import(Import),
    Export(Export),
    Decl(Decl),
}

pub struct Import {
    pub source: String,
    pub specifiers: Vec<ImportSpec>,
    pub kind: ImportKind,         // Value | Type | Namespace
}

pub enum Export {
    Named { name: String },
    NamedValue { name: String, value: Expr },
    Default { expr: Expr },
    Handler { methods: Vec<HandlerMethod> },  // detected route handlers
}

pub struct HandlerMethod {
    pub method: String,           // GET, POST, PUT, DELETE, PATCH
    pub handler: Expr,            // arrow function: (req, ctx) => Response
}
```

### Declarations

```rust
pub enum Decl {
    Function(FunctionDecl),
    Variable(VariableDecl),
    Type(TypeDecl),
    Class(ClassDecl),
    Enum(EnumDecl),
}

pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
    pub is_async: bool,
    pub is_generator: bool,
    pub is_export: bool,
    pub is_default: bool,
}

pub struct VariableDecl {
    pub kind: VarKind,            // Const | Let | Var
    pub pattern: Pat,
    pub init: Option<Expr>,
}

pub struct Param {
    pub pattern: Pat,
    pub type_: Option<Type>,
    pub default: Option<Expr>,
}
```

### Patterns (destructuring)

```rust
pub enum Pat {
    Ident { name: String },
    Array { elems: Vec<Option<Pat>>, rest: Option<Box<Pat>> },
    Object { props: Vec<ObjectPatProp>, rest: Option<Box<Pat>> },
    Rest { arg: Box<Pat> },
    Assign { left: Box<Pat>, right: Expr },
}

pub enum ObjectPatProp {
    KeyValue { key: String, value: Pat },
    Shorthand { name: String },
    Rest { arg: Box<Pat> },
}
```

### Statements

```rust
pub enum Stmt {
    Block(Vec<Stmt>),
    Expr(Expr),
    Var(VariableDecl),
    Return(Option<Expr>),
    If { test: Expr, then: Box<Stmt>, else_: Option<Box<Stmt>> },
    While { test: Expr, body: Box<Stmt> },
    DoWhile { body: Box<Stmt>, test: Expr },
    For { init: Option<ForInit>, test: Option<Expr>, update: Option<Expr>, body: Box<Stmt> },
    ForIn { left: Pat, right: Expr, body: Box<Stmt> },
    ForOf { left: Pat, right: Expr, body: Box<Stmt>, is_await: bool },
    Switch { discriminant: Expr, cases: Vec<SwitchCase> },
    Try { block: Vec<Stmt>, catch: Option<CatchClause>, finally: Option<Vec<Stmt>> },
    Throw(Expr),
    Break(Option<String>),
    Continue(Option<String>),
    Label { label: String, body: Box<Stmt> },
    Debugger,
    Empty,
}

pub enum ForInit {
    Var(VariableDecl),
    Expr(Expr),
}

pub struct SwitchCase {
    pub test: Option<Expr>,       // None = default
    pub body: Vec<Stmt>,
}

pub struct CatchClause {
    pub param: Option<Pat>,
    pub body: Vec<Stmt>,
}

pub struct Block(pub Vec<Stmt>);
```

### Expressions

```rust
pub enum Expr {
    // Literals
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Undefined,
    BigInt(String),
    RegExp { pattern: String, flags: String },
    Template { parts: Vec<TemplatePart>, exprs: Vec<Expr> },
    
    // Identifiers
    Ident(String),
    This,
    Super,
    
    // Operations
    Binary { op: BinaryOp, left: Box<Expr>, right: Box<Expr> },
    Unary { op: UnaryOp, arg: Box<Expr> },
    Update { op: UpdateOp, arg: Box<Expr>, prefix: bool },
    Logical { op: LogicalOp, left: Box<Expr>, right: Box<Expr> },
    Conditional { test: Box<Expr>, then: Box<Expr>, else_: Box<Expr> },
    
    // Access
    Member { object: Box<Expr>, property: Box<Expr>, computed: bool, optional: bool },
    
    // Calls
    Call { callee: Box<Expr>, args: Vec<Expr> },
    New { callee: Box<Expr>, args: Vec<Expr> },
    
    // Functions
    Arrow { params: Vec<Param>, body: ArrowBody, is_async: bool },
    Function(FunctionDecl),
    
    // Objects/Arrays
    Object { props: Vec<Prop> },
    Array { elems: Vec<Option<Expr>> },
    
    // Assignment
    Assign { op: AssignOp, left: Box<Expr>, right: Box<Expr> },
    
    // JSX
    JSX(JSXExpr),
    
    // Async
    Await(Box<Expr>),
    Yield { arg: Option<Box<Expr>>, delegate: bool },
    
    // Types
    TypeCast { expr: Box<Expr>, type_: Type },
    Typeof(Box<Expr>),
    InstanceOf { left: Box<Expr>, right: Box<Expr> },
    In { left: Box<Expr>, right: Box<Expr> },
    
    // Meta
    MetaProp(MetaPropKind),
    Spread(Box<Expr>),
    Sequence(Vec<Expr>),
    
    // Class expression
    Class(ClassDecl),
}

pub enum ArrowBody {
    Expr(Box<Expr>),
    Block(Block),
}

pub enum TemplatePart {
    String(String),
    Expr(Expr),
}
```

### JSX

```rust
pub struct JSXExpr {
    pub name: JSXName,
    pub attrs: Vec<JSXAttr>,
    pub children: Vec<JSXChild>,
    pub is_fragment: bool,
    pub is_self_closing: bool,
    pub key: Option<Expr>,
}

pub enum JSXName {
    Ident(String),                    // div, span
    Member { object: String, property: String },
    Namespaced { ns: String, name: String },
    Dynamic(Box<Expr>),
}

pub enum JSXAttr {
    Regular { name: String, value: Option<JSXAttrValue> },
    Spread(Expr),
    Event { name: String, handler: Expr },    // onClick → on:click
    Directive { name: String, value: Expr },  // use:action, bind:value
}

pub enum JSXAttrValue {
    String(String),
    Expr(Expr),
    Boolean(bool),
}

pub enum JSXChild {
    Text(String),
    Expr(Expr),
    JSX(JSXExpr),
    Fragment(Vec<JSXChild>),
    Spread(Expr),
    Whitespace,                       // significant whitespace
}
```

### Types (TypeScript type system)

```rust
pub enum Type {
    // Primitives
    String, Number, Boolean, Null, Undefined,
    Void, Any, Unknown, Never, BigInt, Symbol,
    
    // References
    Ref { name: String, generics: Vec<Type> },
    Qualified { left: Box<Type>, right: String },
    
    // Composites
    Array { elem: Box<Type> },
    Tuple { elems: Vec<Type> },
    Union { types: Vec<Type> },
    Intersection { types: Vec<Type> },
    Object { members: Vec<ObjectMember> },
    Function { params: Vec<ParamType>, ret: Box<Type>, generics: Vec<GenericParam> },
    
    // Advanced
    Index { object: Box<Type>, index: Box<Type> },
    Conditional { check: Box<Type>, extends: Box<Type>, true_type: Box<Type>, false_type: Box<Type> },
    Mapped { param: String, type_: Box<Type> },
    Paren { type_: Box<Type> },
    Template { parts: Vec<TemplatePart> },
    Literal(LiteralType),
    Infer { param: String },
    Keyof(Box<Type>),                // keyof T
    Typeof(Box<Expr>),               // typeof expr
    Readonly(Box<Type>),             // Readonly<T>
    Partial(Box<Type>),              // Partial<T>
    ArrayReadonly(Box<Type>),        // readonly T[]
    Optional(Box<Type>),             // T?
}

pub struct ObjectMember {
    pub key: String,
    pub type_: Type,
    pub optional: bool,
    pub readonly: bool,
    pub method: bool,               // method signature vs property
    pub getter: bool,
    pub setter: bool,
}

pub struct ParamType {
    pub name: String,
    pub type_: Type,
    pub optional: bool,
    pub rest: bool,
}

pub enum LiteralType {
    String(String),
    Number(f64),
    Bool(bool),
    BigInt(String),
}
```

### Semantic Info (pre-computed)

```rust
pub struct SemanticInfo {
    pub is_route: bool,
    pub is_island: bool,
    pub is_layout: bool,
    pub is_app: bool,
    pub is_middleware: bool,
    pub route_pattern: Option<String>,
    pub handlers: Vec<HandlerMethod>,
    pub hooks: Vec<String>,           // useState, useEffect, etc.
    pub signals: Vec<String>,         // signal, useSignal
    pub components: Vec<String>,      // exported components
    pub islands: Vec<String>,         // island components
    pub imports: Vec<ImportInfo>,
    pub exports: Vec<String>,
}

pub struct ImportInfo {
    pub source: String,
    pub names: Vec<String>,
    pub is_type_only: bool,
}
```

## HIR Builder (oxc → HIR)

The HIR builder uses `oxc_ast_visit::Visit` to traverse the AST in one pass:

```rust
pub struct HIRBuilder<'a> {
    allocator: &'a Allocator,
    module: Module,
    semantic: SemanticInfo,
    errors: Vec<ParseError>,
}

impl<'a> Visit<'a> for HIRBuilder<'a> {
    fn visit_program(&mut self, program: &Program<'a>) {
        for stmt in &program.body {
            self.visit_statement(stmt);
        }
    }
    
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        let import = self.convert_import(decl);
        self.module.items.push(ModuleItem::Import(import));
    }
    
    fn visit_function_declaration(&mut self, func: &Function<'a>) {
        let decl = self.convert_function(func);
        self.module.items.push(ModuleItem::Decl(Decl::Function(decl)));
    }
    
    // ... etc
}
```

## Build Path: HIR → Rust Codegen

```rust
pub struct RustCodegen {
    output: String,
    imports: HashSet<String>,
}

impl RustCodegen {
    pub fn generate_module(&mut self, module: &Module) -> String {
        // 1. Generate imports
        self.generate_imports(&module.semantic);
        
        // 2. Generate type definitions
        for decl in &module.types {
            self.generate_type_decl(decl);
        }
        
        // 3. Generate declarations
        for item in &module.items {
            match item {
                ModuleItem::Decl(decl) => self.generate_decl(decl),
                ModuleItem::Export(export) => self.generate_export(export),
                _ => {}
            }
        }
        
        // 4. Generate route handler wrappers (if route)
        if module.semantic.is_route {
            self.generate_route_wrappers(module);
        }
        
        self.output.clone()
    }
    
    fn generate_jsx(&mut self, jsx: &JSXExpr) -> String {
        if jsx.is_fragment {
            // Fragment → no wrapper
            self.generate_jsx_children(&jsx.children)
        } else if self.is_html_element(&jsx.name) {
            // HTML element → direct VNode creation
            format!(
                "VNode::element(\"{}\"){}.children(vec![{}]).build()",
                self.jsx_name_to_string(&jsx.name),
                self.generate_jsx_attrs(&jsx.attrs),
                self.generate_jsx_children(&jsx.children)
            )
        } else {
            // Component → function call with props
            let props = self.generate_jsx_props(&jsx.attrs, &jsx.children);
            format!("{}({})", self.jsx_name_to_string(&jsx.name), props)
        }
    }
}
```

## Dev Path: HIR → Interpreter

```rust
pub struct HIRInterpreter {
    modules: HashMap<String, Module>,
    components: HashMap<String, FunctionDecl>,
    islands: HashMap<String, IslandDef>,
    hooks: HookRegistry,
}

impl HIRInterpreter {
    pub async fn execute_route(&self, route: &str, method: &str, request: Request) -> Response {
        let module = self.modules.get(route).unwrap();
        let handler = module.semantic.handlers.iter()
            .find(|h| h.method == method)
            .unwrap();
        
        let mut ctx = EvalContext::new(request);
        let result = self.eval_expr(&handler.handler, &mut ctx);
        
        match result {
            Value::VNode(vnode) => Response::html(vnode.render_to_html()),
            Value::Response(resp) => resp,
            other => Response::json(other),
        }
    }
    
    fn eval_expr(&self, expr: &Expr, ctx: &mut EvalContext) -> Value {
        match expr {
            Expr::Call { callee, args } => {
                let callee_val = self.eval_expr(callee, ctx);
                let arg_vals: Vec<Value> = args.iter()
                    .map(|a| self.eval_expr(a, ctx))
                    .collect();
                self.call_function(callee_val, arg_vals, ctx)
            }
            Expr::JSX(jsx) => {
                Value::VNode(self.eval_jsx(jsx, ctx))
            }
            // ... etc
        }
    }
    
    fn eval_jsx(&self, jsx: &JSXExpr, ctx: &mut EvalContext) -> VNode {
        if jsx.is_fragment {
            VNode::Fragment(
                jsx.children.iter()
                    .map(|c| self.eval_jsx_child(c, ctx))
                    .collect()
            )
        } else {
            VNode::Element {
                tag: self.jsx_name_to_string(&jsx.name),
                attrs: self.eval_jsx_attrs(&jsx.attrs, ctx),
                children: jsx.children.iter()
                    .map(|c| self.eval_jsx_child(c, ctx))
                    .collect(),
            }
        }
    }
}
```

## Hot Reload Architecture

```
File change detected
    │
    ▼
┌──────────────┐
│ Re-parse     │  ← oxc_parser (fast, <1ms for typical file)
│ changed file │
└──────────────┘
    │
    ▼
┌──────────────┐
│ Re-build HIR │  ← HIRBuilder::visit_program
└──────────────┘
    │
    ▼
┌──────────────┐
│ Update       │  ← swap module in interpreter state
│ interpreter  │
└──────────────┘
    │
    ▼
┌──────────────┐
│ Notify       │  ← WebSocket / SSE to browser
│ browser      │
└──────────────┘
    │
    ▼
Browser re-fetches island bundles
```

## Caching Strategy

```rust
// Incremental build cache
pub struct BuildCache {
    // HIR cache: path → (mtime, HIR module)
    hir_cache: HashMap<PathBuf, (SystemTime, Module)>,
    
    // Dependency graph: module → [deps]
    deps: HashMap<PathBuf, Vec<PathBuf>>,
    
    // Rust codegen cache: HIR hash → generated Rust code
    codegen_cache: HashMap<u64, String>,
}

impl BuildCache {
    pub fn get_or_parse(&mut self, path: &Path) -> Result<&Module> {
        let mtime = fs::metadata(path)?.modified()?;
        
        if let Some((cached_mtime, module)) = self.hir_cache.get(path) {
            if *cached_mtime == mtime {
                return Ok(module);
            }
        }
        
        // Parse and build HIR
        let source = fs::read_to_string(path)?;
        let module = HIRBuilder::parse(&source, path)?;
        self.hir_cache.insert(path.to_path_buf(), (mtime, module));
        Ok(&self.hir_cache.get(path).unwrap().1)
    }
}
```

## Island Boundary Detection

Islands are detected during HIR building by analyzing imports and hook usage:

```rust
fn detect_islands(module: &Module) -> Vec<IslandDef> {
    let mut islands = Vec::new();
    
    for item in &module.items {
        if let ModuleItem::Decl(Decl::Function(func)) = item {
            // Island criteria:
            // 1. Uses hooks (useState, useEffect, useSignal, etc.)
            // 2. Has JSX return
            // 3. Located in islands/ directory
            let uses_hooks = func.body.iter()
                .any(|stmt| contains_hook_call(stmt));
            let returns_jsx = returns_jsx_type(&func.return_type);
            
            if uses_hooks || (module.source.contains("islands/") && returns_jsx) {
                islands.push(IslandDef {
                    name: func.name.clone(),
                    props: func.params.clone(),
                    client_bundle: format!("islands/{}.js", func.name),
                });
            }
        }
    }
    
    islands
}
```

## Key Benefits

1. **Correctness**: oxc_parser handles all TS/TSX edge cases (UTF-8, template literals, type annotations, etc.)
2. **Performance**: Single-pass HIR builder, cached HIR for incremental builds
3. **Simplicity**: Interpreter works on HIR directly, no Rust compilation in dev
4. **Flexibility**: HIR can be serialized, transformed, analyzed independently
5. **Type safety**: TypeScript types preserved in HIR for codegen, but semantically erased for interpreter
