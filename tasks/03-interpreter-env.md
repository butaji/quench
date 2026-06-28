# Task 03: Implement scope, closures, and interpreter eval loop

## Goal

Evaluate the AST produced by Task 02 with proper variable scoping and first-class functions.

## Files

- Create: `src/js_runtime/env.rs`
- Create: `src/js_runtime/interpreter.rs`
- Modify: `src/js_runtime/mod.rs` to wire eval through the interpreter.

## Steps

1. Implement `env.rs`:
   - `pub struct Scope` mapping names to `Rc<RefCell<Value>>`.
   - `pub struct Environment` with a scope chain and `get`/`set`/`declare`/`push_scope`/`pop_scope`.
2. Implement `interpreter.rs`:
   - `pub fn eval(program: &Program, env: &mut Environment) -> Result<Value, JsError>`.
   - Expression eval: literals, identifiers, member access, binary/unary operators, calls.
   - Statement eval: block, var/let/const, assignment, if/while/for, return, break/continue.
   - Function values must capture their definition environment as a closure.
   - Native functions are `Value::NativeFunction(fn(&[Value]) -> Result<Value>)`.
3. Add unit tests for:
   - arithmetic and comparisons
   - nested functions and closures
   - recursion
   - early `return`

## Boundaries

- Only implement evaluation logic. Do not add bridge calls, globals, or built-ins here.
- Do not modify any module outside `src/js_runtime/`.

## Acceptance criteria

- `cargo test js_runtime::interpreter` passes.
- A JS snippet defining and calling a recursive factorial function returns the correct value.
- A closure retains captured variables after the outer function returns.

## Verification

```bash
cargo test js_runtime::interpreter
```
