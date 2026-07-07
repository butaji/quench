> **Exact implementation plan for ES6 class support.** The background process is applying this plan; update this task as each piece lands.

# Task 182: Class support and conformance skip fixes

## Status: IN PROGRESS

## Gap

JavaScript class syntax (`class`, `extends`, `super`, `static`) is not implemented. Class-related test262 and TypeScript conformance cases are skipped or fail.

## Exact implementation

Do not explore alternatives. Implement classes exactly as follows:

### 1. AST types (`crates/quench-runtime/src/ast.rs`)

Add after the `Span` struct:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    Constructor { params: Vec<String>, body: Vec<Statement> },
    Method { name: PropertyKey, params: Vec<String>, body: Vec<Statement>, is_static: bool },
    Getter { name: PropertyKey, body: Vec<Statement> },
    Setter { name: PropertyKey, param: String, body: Vec<Statement> },
    Field { name: PropertyKey, init: Option<Expression>, is_static: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub name: Option<String>,
    pub super_class: Option<Box<Expression>>,
    pub members: Vec<ClassMember>,
}
```

Add to `Statement`:

```rust
ClassDeclaration { name: String, class: Box<Class> },
```

Add to `Expression`:

```rust
Class(Box<Class>),
```

### 2. Lowering (`crates/quench-runtime/src/lower.rs`)

In `lower_decl`, handle `swc::Decl::Class`:

```rust
swc::Decl::Class(class_decl) => {
    let name = class_decl.ident.sym.to_string();
    let class = lower_class(&class_decl.class)?;
    Some(Statement::ClassDeclaration { name, class: Box::new(class) })
}
```

Add `lower_class` and `lower_class_member` helpers.

In `lower_expr`, handle `swc::Expr::Class`:

```rust
swc::Expr::Class(class_expr) => {
    let class = lower_class(&class_expr.class)?;
    Ok(Expression::Class(Box::new(class)))
}
```

### 3. Interpreter (`crates/quench-runtime/src/interpreter/*.rs`)

In statement dispatch:

```rust
Statement::ClassDeclaration { name, class } => {
    let class_value = eval_class(class, env)?;
    env.borrow_mut().define(name.clone(), class_value);
    Ok(Value::Undefined)
}
```

In expression dispatch:

```rust
Expression::Class(class) => eval_class(class, env),
```

Add `eval_class` and `property_key_to_string`. The class constructor must:

1. Create a function object for the class.
2. Set `prototype` to a new object whose `[[Prototype]]` is the superclass prototype (if `extends`).
3. Attach instance methods/getters/setters/fields to `prototype`.
4. Attach static members to the constructor function itself.
5. Return the constructor as the class value.

### 4. Value model (`crates/quench-runtime/src/value.rs`)

Add to `ValueFunction`:

```rust
pub fn set_prototype(&mut self, proto: Rc<RefCell<Object>>) {
    *self.proto_cell.borrow_mut() = Some(proto);
}
```

## Acceptance criteria

- [ ] `class A {}` creates a constructor function with correct prototype.
- [ ] `class B extends A {}` sets up inheritance and `instanceof` works.
- [ ] `super()` and `super.method()` work inside class methods.
- [ ] Static methods and fields are attached to the constructor.
- [ ] Regression tests and JS scenario tests for class behavior.

## Files

- `crates/quench-runtime/src/ast.rs`
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/interpreter/*.rs`
- `crates/quench-runtime/src/value.rs`

## Tests unblocked

- test262 `language/class/`
- TypeScript class conformance cases

## Targets

- **Suite:** `both`
- **Batch:** 4
- **Target subset:** `tests/test262/test/language/class/` + TypeScript class conformance cases
- **Blocked by:** 85
- **Exit criteria:** Class syntax subsets pass at 100% with zero spec skips.
