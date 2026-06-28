# Task 03: Implement scope, closures, and interpreter eval loop

## Goal

Evaluate the AST produced by Task 02 with proper variable scoping and first-class functions, all inside the `quench-runtime` crate.

> **Custom vs crate:** This task writes the core execution engine (scope chain, closures, eval loop). No replacement crate is used here — this is the custom part of the runtime.

## Files

- Create: `crates/quench-runtime/src/env.rs`
- Create: `crates/quench-runtime/src/interpreter.rs`
- Modify: `crates/quench-runtime/src/lib.rs` to wire eval through the interpreter.

## Steps

1. Implement `env.rs`:
   - `pub struct Scope` mapping interned string ids to `Rc<RefCell<Value>>`.
   - `pub struct Environment` with a scope chain and `get`/`set`/`declare`/`push_scope`/`pop_scope`.
2. Implement `interpreter.rs`:
   - `pub fn eval(program: &Program, env: &mut Environment) -> Result<Value, JsError>`.
   - Expression eval: literals, identifiers, member access, binary/unary operators, calls.
   - Statement eval: block, var/let/const, assignment, if/while/for, return, break/continue.
   - Function values must capture their definition environment as a closure.
   - Native functions are `Value::NativeFunction(Box<dyn Fn(&[Value]) -> Result<Value>>)`.
3. Add unit tests for:
   - arithmetic and comparisons
   - nested functions and closures
   - recursion
   - early `return`

## Boundaries

- Only implement evaluation logic. Do not add bridge calls, globals, or built-ins here.
- Do not modify any module outside `crates/quench-runtime/`.

## Acceptance criteria

- `cargo test -p quench-runtime interpreter` passes.
- A JS snippet defining and calling a recursive factorial function returns the correct value.
- A closure retains captured variables after the outer function returns.

## Verification

```bash
cargo test -p quench-runtime interpreter
```
