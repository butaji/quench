> **Canonical fix for the recursive-interpreter stack overflow.**

# Task 85: Implement trampoline interpreter with explicit call stack

## Goal

Replace the recursive interpreter with a trampoline loop and a heap-allocated `Vec<CallFrame>` so JS recursion no longer consumes the native Rust stack.

## Why

The current interpreter calls itself recursively for every JS function call, loop body, and nested expression. Deep JS recursion exhausts the native Rust stack. Task 338 fixes the false stack-overflow errors caused by a global depth counter; this task fixes the real stack consumption by replacing recursion with a heap-allocated call stack.

- Rust stack depth stays O(1).
- JS stack depth is tracked on the heap via `Vec<CallFrame>`.
- Runaway recursion throws a controlled JS `RangeError` at `MAX_JS_STACK`.

## Rust-specific design constraints

The trampoline rewrite is the right place to remove `Rc<RefCell<...>>` from the hot path and let Rust's ownership model enforce VM safety:

- Pass `&mut Context` through `step_frame`. The `Context` owns the object arena, global environment, and call stack.
- Store JS objects in a slot-indexed arena (`Vec<Object>` or `SlotMap`) referenced by `ObjectId`. Property access becomes a bounds-checked array lookup, not a runtime borrow check.
- Keep `Value` as a small enum with no vtables; dispatch through `match`.
- Keep `unsafe` isolated to value representation (e.g., NaN boxing) and verify with Miri.

## Design

### `CallFrame`

```rust
struct CallFrame {
    func: FunctionId,          // function being executed
    pc: usize,                 // next AST node index
    env: Environment,          // lexical environment
    operands: Vec<Value>,      // expression operand stack
    return_to: Option<usize>,  // parent frame index
}
```

### `Action`

```rust
enum Action {
    Continue,
    Call { callee: FunctionId, args: Vec<Value> },
    TailCall { callee: FunctionId, args: Vec<Value> },
    Return(Value),
    Throw(Value),
}
```

### Trampoline loop

```rust
fn run_trampoline(entry: FunctionId, args: Vec<Value>, global: Environment) -> Result<Value, JsError> {
    let mut stack = vec![CallFrame::new(entry, args, global)];
    let mut current_result: Option<Value> = None;

    loop {
        match stack.last_mut() {
            None => return Ok(current_result.unwrap_or(Value::Undefined)),
            Some(frame) => {
                match step_frame(frame, &mut current_result)? {
                    Action::Continue => {}
                    Action::Return(v) => { current_result = Some(v); stack.pop(); }
                    Action::Call { callee, args } => {
                        if stack.len() >= MAX_JS_STACK {
                            return Err(JsError("RangeError: Maximum call stack size exceeded".into()));
                        }
                        stack.push(CallFrame::new(callee, args));
                        current_result = None;
                    }
                    Action::TailCall { callee, args } => {
                        *frame = CallFrame::new(callee, args);
                        current_result = None;
                    }
                    Action::Throw(e) => {
                        // unwind stack until a try/catch frame is found
                        todo!("implement exception unwinding");
                    }
                }
            }
        }
    }
}
```

### `step_frame`

Executes a single AST node, updates `frame.pc`, manipulates `frame.operands`, and returns an `Action`. It must never call another `eval` recursively.

## Tail calls

During lowering, mark calls that are in tail position. The interpreter emits `Action::TailCall` for those, replacing the current frame instead of pushing a new one. This gives free TCO for tail-recursive JS functions.

## Exception handling

`try/catch/finally` is implemented by:

1. Recording a try-frame with the catch handler and finally block addresses.
2. On `Action::Throw`, pop frames until a try-frame is found.
3. Execute catch or finally as appropriate.

## Generators / async (future)

With an explicit stack, `yield` and `await` become saving the `Vec<CallFrame>` into a generator/promise object and resuming later. This task does not implement them, but the design must not block them.

## Files

- `crates/quench-runtime/src/interpreter/trampoline.rs` (new)
- `crates/quench-runtime/src/interpreter/frame.rs` (new)
- `crates/quench-runtime/src/interpreter/mod.rs`
- `crates/quench-runtime/src/ast.rs` (tail-call marks)
- `crates/quench-runtime/src/lower/` (tail-position analysis)
- `crates/quench-runtime/src/interpreter.rs` (remove recursive eval once trampoline is complete; depth counter is handled by Task 338)

## Acceptance criteria

- All existing tests pass.
- A new regression test runs deeply recursive JS without native stack overflow:
  ```js
  function f(n) { if (n === 0) return 0; return 1 + f(n - 1); }
  f(100000);
  ```
- A new regression test for tail-call optimization:
  ```js
  function sum(n, acc) { if (n === 0) return acc; return sum(n - 1, acc + n); }
  sum(100000, 0);
  ```
- Conformance harnesses can run larger subsets without crashing from stack overflow.

## Boundaries

- Only modify `crates/quench-runtime/src/` and `Cargo.toml`.
- Do not touch `src/bridge/`, `src/ink/`, `src/render/`, `src/compiler/`.
- `examples/` remain immutable.

## Verification

```bash
cargo test -p quench-runtime
cargo test -p quench-runtime --test conformance test_typescript_conformance_expressions -- --ignored --nocapture
cargo test -p quench-runtime --test test262 test262_expressions -- --ignored --nocapture
```

## Targets

- **Suite:** `test262`
- **Batch:** 1
- **Target subset:** n/a (interpreter infrastructure)
- **Blocked by:** 338
- **Exit criteria:** Recursive stress test (`f(100000)`) passes without native stack overflow and all existing tests pass.

## Status

`pending`.
