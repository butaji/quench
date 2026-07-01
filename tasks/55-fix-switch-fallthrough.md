# Task 55: Fix switch fallthrough and labeled break

## Goal

Fix the switch fallthrough and labeled break bugs that caused 5 test failures in `context::tests::stray_tests`.

## Bug Description

The 5 failing tests were:
- `test_switch_no_fallthrough`
- `test_switch_fallthrough`
- `test_switch_default_fallthrough`
- `test_switch_multiple_fallthroughs`
- `test_labeled_break`

All failed with `Err(JsError(Break))` - the break statement was not being caught by the switch/labeled block.

## Root Cause

The `eval_labeled` function in `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs` did NOT intercept break/continue errors. It pushed the label onto the stack, evaluated the body, popped the label, and returned the result. But when the body returned `Err(JsError::Break(_))`, it just returned that error without checking if the break should be handled by this label.

The flow was:
1. `break;` executes → `eval_statement` returns `Err(JsError::Break(None))`
2. This error bubbles up through `eval_if` (in the switch case) and `eval_block`
3. `eval_labeled` catches the error, pops its label, and returns the error
4. The break never gets handled!

## Fix Applied

Modified `eval_labeled` to intercept break errors after evaluating the body:

```rust
fn eval_labeled(
    label: &str,
    body: &Box<Statement>,
    env: &Rc<RefCell<Environment>>,
    is_expr_body: bool,
    is_function_body: bool,
) -> Result<Value, JsError> {
    push_labeled_target(label);
    let result = eval_statement(body, env, is_expr_body, is_function_body);
    pop_labeled_target();

    // Handle break statements that target this label
    if let Err(JsError::Break(break_label)) = &result {
        // If break has no label (unlabeled break), it targets this block
        // If break has a label that matches this label, it targets this block
        if break_label.as_ref().map_or(true, |l| l == label) {
            return Ok(Value::Undefined); // Break handled
        }
        // Otherwise, propagate (labeled break targeting outer block)
    }

    result
}
```

## Files Modified

- `crates/quench-runtime/src/interpreter/eval_stmt/mod.rs`

## Verification

```bash
cargo test -p quench-runtime -- context::tests::stray_tests
# All 5 tests now pass

cargo test
# All 34 main tests pass
# All 5 parity tests pass

cargo run --release -- examples/simple.js
cargo run --release -- examples/counter.js
```

## Status: COMPLETED

- Fix applied and all tests pass.
- Regression tests verified: switch fallthrough and labeled break work correctly.
