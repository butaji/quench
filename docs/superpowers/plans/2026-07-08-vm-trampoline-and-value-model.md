# VM Trampoline + Unified Value Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or inline execution. This is a large foundation refactor; work in small, testable milestones.

**Goal:** Replace the recursive/Task-Cont interpreter with an explicit `Vec<CallFrame>` trampoline, move objects into a slot-indexed arena, and collapse functions/native functions/constructors into `Value::Object` with internal `[[Call]]`/`[[Construct]]` slots.

**Architecture:** A single `Context` owns an object arena (`Vec<Object>`) and the JS call stack (`Vec<CallFrame>`). `Value` becomes primitives + `Object(ObjectId)`. Functions are objects whose `ObjectKind` carries `call`/`construct` closures. `step_frame` mutates the top frame and returns an `Action` (`Continue`, `Call`, `TailCall`, `Return`, `Throw`); the trampoline loop applies the action without consuming the native Rust stack.

**Tech Stack:** Rust, `crates/quench-runtime`, existing `ast` module.

---

## File Structure

- **Create** `crates/quench-runtime/src/interpreter/frame.rs` — `CallFrame`, `Action`, `FrameState`, operand-stack helpers.
- **Create** `crates/quench-runtime/src/interpreter/trampoline.rs` — trampoline loop, `step_frame`, `run_trampoline`.
- **Create** `crates/quench-runtime/src/arena.rs` — object arena, `ObjectId`, allocation helpers.
- **Modify** `crates/quench-runtime/src/value.rs` — collapse to primitives + `Object(ObjectId)`; add `[[Call]]`/`[[Construct]]` slots.
- **Modify** `crates/quench-runtime/src/object.rs` (currently `value.rs`) — add `call`/`construct` closures and `ObjectKind` variants.
- **Modify** `crates/quench-runtime/src/env.rs` — store `Value` bindings, remove `Rc<RefCell>` where possible.
- **Modify** `crates/quench-runtime/src/interpreter/mod.rs` — expose new entry points.
- **Modify** `crates/quench-runtime/src/builtins.rs` — return `ObjectId`/handles using arena.
- **Modify** `crates/quench-runtime/src/lib.rs` — `Context` owns arena and call stack, remove `globals` HashMap.
- **Modify** `crates/quench-runtime/src/interpreter.rs` — delete or archive old recursive/Task-Cont code once new interpreter is wired.

---

## Task 1: Bootstrap Arena and New Value Skeleton

**Files:**
- Create: `crates/quench-runtime/src/arena.rs`
- Modify: `crates/quench-runtime/src/value.rs`
- Modify: `crates/quench-runtime/src/lib.rs`

- [ ] **Step 1.1: Define `ObjectId` and `Arena`**

```rust
// crates/quench-runtime/src/arena.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(pub usize);

pub struct Arena<T> {
    slots: Vec<T>,
}

impl<T> Arena<T> {
    pub fn new() -> Self { Self { slots: Vec::new() } }
    pub fn alloc(&mut self, value: T) -> ObjectId {
        let id = self.slots.len();
        self.slots.push(value);
        ObjectId(id)
    }
    pub fn get(&self, id: ObjectId) -> Option<&T> { self.slots.get(id.0) }
    pub fn get_mut(&mut self, id: ObjectId) -> Option<&mut T> { self.slots.get_mut(id.0) }
}
```

- [ ] **Step 1.2: Shrink `Value` to primitives + object reference**

Replace `Value::Object`, `Value::Function`, `Value::NativeFunction`, `Value::NativeConstructor` with a single object reference. Keep `Symbol` as a primitive-like variant for now.

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Symbol(&'static str), // interned later; string tag for now
    Object(ObjectId),
}
```

Update `PartialEq`, `Display`, `Debug` to match. Functions will be objects, so `Debug` for `Value::Object(id)` can look up the arena and print `[Function: ...]` or `[Object ...]` based on `ObjectKind`.

- [ ] **Step 1.3: Make `Context` own the arena**

```rust
pub struct Context {
    pub arena: Arena<Object>,
    pub env: Environment,
    pub call_stack: Vec<CallFrame>,
}
```

Add `alloc_object(&mut self, obj: Object) -> ObjectId`. Remove `globals: HashMap<...>` (Task 286 will be folded here).

- [ ] **Step 1.4: Add a compile-only test**

Run: `cargo check -p quench-runtime`
Expected: compile errors in dependent modules (expected; next tasks fix them).

---

## Task 2: Refactor `Object` to Carry Call/Construct Slots

**Files:**
- Modify: `crates/quench-runtime/src/value.rs`

- [ ] **Step 2.1: Add internal slots to `Object`**

```rust
pub struct Object {
    pub properties: HashMap<String, Value>,
    pub elements: Vec<Value>,
    pub kind: ObjectKind,
    pub prototype: Option<ObjectId>,
    getters: HashMap<String, GetterStorage>,
    setters: HashMap<String, SetterStorage>,
    pub call: Option<CallSlot>,
    pub construct: Option<ConstructSlot>,
}

pub enum CallSlot {
    Js(JsFunction),
    Native(NativeFunction),
}

pub enum ConstructSlot {
    Js(JsFunction),
    Native(NativeConstructor),
}

pub struct JsFunction {
    pub name: Option<String>,
    pub params: Vec<String>,
    pub body: Vec<crate::ast::Statement>,
    pub arrow_body: Option<Box<crate::ast::ArrowBody>>,
    pub closure: Environment,
    pub is_arrow: bool,
}
```

- [ ] **Step 2.2: Update object constructors**

`Object::new` and `Object::with_prototype` initialize `call: None`, `construct: None`. `Object::new_array` stays as-is except returning `Object` (not `Rc<RefCell<Object>>`).

- [ ] **Step 2.3: Compile check**

Run: `cargo check -p quench-runtime`
Expected: more errors in interpreter/builtins; fix signature mismatches in next tasks.

---

## Task 3: Implement CallFrame and Trampoline Skeleton

**Files:**
- Create: `crates/quench-runtime/src/interpreter/frame.rs`
- Create: `crates/quench-runtime/src/interpreter/trampoline.rs`
- Modify: `crates/quench-runtime/src/interpreter/mod.rs`

- [ ] **Step 3.1: Define `CallFrame`**

```rust
pub struct CallFrame {
    pub function_id: ObjectId,
    pub pc: usize,
    pub env: Environment,
    pub operands: Vec<Value>,
    pub return_to: Option<usize>,
    pub this_value: Value,
    pub is_expr_body: bool,
}
```

- [ ] **Step 3.2: Define `Action`**

```rust
pub enum Action {
    Continue,
    Call { callee: ObjectId, args: Vec<Value>, this_value: Value },
    TailCall { callee: ObjectId, args: Vec<Value>, this_value: Value },
    Return(Value),
    Throw(Value),
}
```

- [ ] **Step 3.3: Write trampoline loop**

```rust
pub fn run_trampoline(
    ctx: &mut Context,
    entry: ObjectId,
    args: Vec<Value>,
    this_value: Value,
) -> Result<Value, JsError> {
    const MAX_JS_STACK: usize = 100_000;
    ctx.call_stack.push(CallFrame::new(entry, args, this_value, ctx.env.clone()));

    let mut result: Option<Value> = None;
    loop {
        let action = step_frame(ctx)?;
        match action {
            Action::Continue => {}
            Action::Return(v) => {
                result = Some(v);
                if ctx.call_stack.len() <= 1 {
                    ctx.call_stack.pop();
                    return Ok(v);
                }
                let frame_idx = ctx.call_stack.len() - 1;
                let parent_idx = ctx.call_stack[frame_idx].return_to
                    .expect("return_to missing for non-top frame");
                ctx.call_stack.pop();
                ctx.call_stack[parent_idx].operands.push(v);
            }
            Action::Call { callee, args, this_value } => {
                if ctx.call_stack.len() >= MAX_JS_STACK {
                    return Err(JsError("RangeError: Maximum call stack size exceeded".into()));
                }
                let return_to = ctx.call_stack.len() - 1;
                ctx.call_stack.push(CallFrame::new(callee, args, this_value, Environment::with_parent(...)));
                ctx.call_stack.last_mut().unwrap().return_to = Some(return_to);
            }
            Action::TailCall { callee, args, this_value } => {
                *ctx.call_stack.last_mut().unwrap() = CallFrame::new(callee, args, this_value, ...);
            }
            Action::Throw(e) => {
                // unwind until try-frame (future task; for now propagate)
                return Err(JsError(format!("Uncaught exception: {}", e)));
            }
        }
    }
}
```

- [ ] **Step 3.4: Stub `step_frame`**

```rust
fn step_frame(ctx: &mut Context) -> Result<Action, JsError> {
    let frame_idx = ctx.call_stack.len() - 1;
    let frame = &mut ctx.call_stack[frame_idx];
    // dispatch by pc and AST kind
    Err(JsError("step_frame stub".into()))
}
```

- [ ] **Step 3.5: Wire module exports**

In `interpreter/mod.rs`, re-export `run_trampoline` and `CallFrame`. Remove old `eval_program`/`eval_expression` re-exports.

- [ ] **Step 3.6: Compile check**

Run: `cargo check -p quench-runtime`
Expected: compile errors in builtins/lib where old `Value` API is used.

---

## Task 4: Port Expression Evaluation to Stack Machine

**Files:**
- Modify: `crates/quench-runtime/src/interpreter/trampoline.rs`

- [ ] **Step 4.1: Implement literal and identifier expressions**

`Expression::Number(n)` → push `Value::Number(n)`.
`Expression::String(s)` → push `Value::String(s)`.
`Expression::Boolean(b)` → push `Value::Boolean(b)`.
`Expression::Null` → push `Value::Null`.
`Expression::Undefined` → push `Value::Undefined`.
`Expression::Identifier(name)` → look up in frame env; push value or TDZ error.

- [ ] **Step 4.2: Implement binary expressions**

Push `Cont::BinaryRight { op, left }` onto the frame state (a small continuation stack local to the frame), evaluate left, then right, then compute.

- [ ] **Step 4.3: Implement unary expressions**

Similar to binary but single operand.

- [ ] **Step 4.4: Implement member access and assignment**

`Expression::Member` evaluates object then property (if computed), then performs prototype-chain lookup using the arena.
`Expression::Assignment` evaluates RHS, then assigns to identifier or member.

- [ ] **Step 4.5: Implement call expressions**

`Expression::Call` evaluates callee and arguments, then returns `Action::Call` (or `Action::TailCall` if in tail position; tail analysis can be added in a follow-up).

- [ ] **Step 4.6: Compile and unit test**

Run: `cargo test -p quench-runtime --test runtime_issues test_eval_returns_expression_value`
Expected: PASS once basic expressions work.

---

## Task 5: Port Statement Evaluation

**Files:**
- Modify: `crates/quench-runtime/src/interpreter/trampoline.rs`

- [ ] **Step 5.1: Implement expression and block statements**

`Statement::Expression` → evaluate expression, pop result.
`Statement::Block` → push new scope, evaluate statements, pop scope.

- [ ] **Step 5.2: Implement var/let/const declarations with hoisting**

Call `hoist_functions`, `hoist_var_declarations`, `hoist_let_const` at frame entry (or on first step) before executing statements.

- [ ] **Step 5.3: Implement if/while/for**

Use frame-local continuation state to evaluate condition, then body, then loop back.

- [ ] **Step 5.4: Implement return, break, continue**

`return` → return `Action::Return(value)`.
`break`/`continue` → store in frame state and propagate up the statement list.

- [ ] **Step 5.5: Run scenario tests**

Run: `cargo test -p quench-runtime --test scenarios`
Expected: all scenario tests pass.

---

## Task 6: Port Function Calls and Native Functions

**Files:**
- Modify: `crates/quench-runtime/src/interpreter/trampoline.rs`
- Modify: `crates/quench-runtime/src/builtins.rs`
- Modify: `crates/quench-runtime/src/host.rs`

- [ ] **Step 6.1: Implement `Action::Call` dispatch**

Look up callee object in arena. If `call` slot is `CallSlot::Js`, create new `CallFrame` with bound params and parent closure. If `CallSlot::Native`, call the native closure immediately and push result.

- [ ] **Step 6.2: Implement `new` operator**

`Expression::New` evaluates constructor, allocates a new object with constructor's `prototype`, then dispatches `Action::Call` with the new object as `this_value`. If constructor has `construct` slot, use that; otherwise fall back to `call`.

- [ ] **Step 6.3: Update builtins to allocate objects in arena**

Replace `Rc<RefCell<Object>>` creation with `ctx.alloc_object(Object::new(...))`. Native function signatures become `Fn(&mut Context, Vec<Value>) -> Result<Value, JsError>`.

- [ ] **Step 6.4: Run function tests**

Run: `cargo test -p quench-runtime --test var_hoisting_tdz`
Expected: all 18 tests pass.

---

## Task 7: Port Exception Handling

**Files:**
- Modify: `crates/quench-runtime/src/interpreter/trampoline.rs`

- [ ] **Step 7.1: Add try-frame metadata to `CallFrame`**

```rust
pub struct TryFrame {
    pub catch_pc: usize,
    pub finally_pc: Option<usize>,
    pub param: Option<String>,
    pub saved_stack_len: usize,
}
```

- [ ] **Step 7.2: Implement throw/catch unwind**

On `Action::Throw`, pop frames until one with a `TryFrame` is found, set PC to catch handler, bind param in env, and push exception. If none found, return error.

- [ ] **Step 7.3: Run throw tests**

Run: `cargo test -p quench-runtime --test scenarios scenario_throw_error_object`
Expected: PASS.

---

## Task 8: Remove Old Interpreter and Thread-Locals

**Files:**
- Delete or archive: `crates/quench-runtime/src/interpreter.rs`
- Modify: `crates/quench-runtime/src/interpreter/mod.rs`

- [ ] **Step 8.1: Remove `interpreter.rs` old code**

Move any still-useful helpers (e.g., `to_number`, `to_bool`) into `value.rs` or `interpreter/helpers.rs`. Delete the recursive `eval_*` functions and thread-locals `CONTROL_FLOW`, `CURRENT_THIS`, `RETURN_VALUE`, `CURRENT_DEPTH`.

- [ ] **Step 8.2: Update `Context::eval`**

```rust
pub fn eval(&mut self, source: &str) -> Result<Value, JsError> {
    let program = self.parse(source)?;
    let main = self.allocate_script_function(program)?;
    run_trampoline(self, main, vec![], Value::Undefined)
}
```

- [ ] **Step 8.3: Compile check**

Run: `cargo check -p quench-runtime`
Expected: clean compile.

---

## Task 9: Add Regression Tests for Deep Recursion and TCO

**Files:**
- Modify: `crates/quench-runtime/tests/runtime_issues.rs`

- [ ] **Step 9.1: Add deep recursion test**

```rust
#[test]
fn test_deep_recursion_100000_does_not_overflow() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        function f(n) { if (n === 0) return 0; return 1 + f(n - 1); }
        f(100000);
    "#).unwrap();
    assert_eq!(result, Value::Number(100000.0));
}
```

- [ ] **Step 9.2: Add tail-call test**

```rust
#[test]
fn test_tail_call_100000_does_not_overflow() {
    let mut ctx = Context::new().unwrap();
    let result = ctx.eval(r#"
        function sum(n, acc) { if (n === 0) return acc; return sum(n - 1, acc + n); }
        sum(100000, 0);
    "#).unwrap();
    assert_eq!(result, Value::Number(5000050000.0));
}
```

- [ ] **Step 9.3: Run regression tests**

Run: `cargo test -p quench-runtime --test runtime_issues`
Expected: PASS.

---

## Task 10: Collapse Value Model (Task 335)

**Files:**
- Modify: `crates/quench-runtime/src/value.rs`
- Modify: `crates/quench-runtime/src/interpreter/trampoline.rs`
- Modify: `crates/quench-runtime/src/builtins.rs`

- [ ] **Step 10.1: Verify no remaining `Value::Function`/`NativeFunction`/`NativeConstructor` variants**

Grep: `grep -n "Value::Function\|Value::NativeFunction\|Value::NativeConstructor" crates/quench-runtime/src/`
Expected: no matches outside tests/docs.

- [ ] **Step 10.2: Update helper functions**

`to_string`, `to_number`, `to_bool`, `strict_eq`, `loose_eq`, `typeof` must dispatch on `Value::Object(id)` and inspect `arena.get(id).kind`.

- [ ] **Step 10.3: Update instanceof**

Compare prototype chains using `ObjectId`.

- [ ] **Step 10.4: Run full runtime suite**

Run: `cargo test -p quench-runtime`
Expected: all tests pass.

---

## Task 11: Remove `Context::globals` (Task 286 Folded In)

**Files:**
- Modify: `crates/quench-runtime/src/lib.rs`

- [ ] **Step 11.1: Delete `globals` field and methods**

`set_global` writes directly to top-level env. `get_global` reads from top-level env.

- [ ] **Step 11.2: Update tests**

`test_globals` and `test_context_creation` still pass using env lookups.

- [ ] **Step 11.3: Compile and test**

Run: `cargo test -p quench-runtime`
Expected: PASS.

---

## Task 12: Verify Examples and Full Workspace

**Files:**
- N/A (verification)

- [ ] **Step 12.1: Run runtime examples**

```bash
cargo run -- examples/counter.js
cargo run -- examples/use-bridge.tsx --prop theme=dark
cargo run -- examples/animations.tsx
```
Expected: each completes without stack overflow or initialization errors.

- [ ] **Step 12.2: Run workspace tests**

```bash
cargo test --workspace
```
Expected: all test results show `0 failed`.

---

## Self-Review Checklist

- [ ] Spec coverage: Task 85 trampoline, Task 335 value collapse, Task 286 globals removal all have tasks.
- [ ] No placeholders: every step has concrete code or exact command.
- [ ] Type consistency: `ObjectId`, `Value`, `CallFrame`, `Action` used consistently across tasks.
- [ ] Testing: each milestone has a focused test command.
