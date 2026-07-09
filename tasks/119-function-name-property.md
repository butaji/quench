# Task 119: Implement function name property

## Status: COMPLETED

## Implementation

The function name property is implemented in `eval/member.rs` via `eval_function_member`:

```rust
pub fn eval_function_member(f: &crate::value::ValueFunction, prop_name: &str) -> Result<Value, JsError> {
    // ...
    match prop_name {
        "name" => Ok(Value::String(f.name.clone().unwrap_or_default())),
        // ...
    }
}
```

The `ValueFunction` struct already has a `name` field that is populated when functions are created:
- Function declarations: `ValueFunction::new(Some(name.clone()), ...)` in `interpreter.rs:155`
- Function expressions: `ValueFunction::new(name.clone(), ...)` in `eval/expression.rs:45`
- Arrow functions: `ValueFunction::new_arrow(...)` (no name for arrows)

## Acceptance criteria

- [x] `(function foo() {}).name` returns "foo"
- [x] `(function() {}).name` returns ""
- [x] `function bar() {}; bar.name` returns "bar"
- [x] Regression tests added in `tests/runtime_issues.rs::test_function_name_property`

## Tests

Added `test_function_name_property` test in `crates/quench-runtime/tests/runtime_issues.rs`:

```rust
#[test]
fn test_function_name_property() {
    let mut ctx = Context::new().unwrap();
    
    // Named function expression
    let result = ctx.eval("(function foo() {}).name").unwrap();
    assert_eq!(result.to_string(), "foo", "Named function expression should have name 'foo'");
    
    // Anonymous function expression
    let result = ctx.eval("(function() {}).name").unwrap();
    assert_eq!(result.to_string(), "", "Anonymous function expression should have empty name");
    
    // Named function declaration
    let result = ctx.eval("function bar() {}; bar.name").unwrap();
    assert_eq!(result.to_string(), "bar", "Named function declaration should have name 'bar'");
}
```

## Files

- `crates/quench-runtime/src/eval/member.rs` - `eval_function_member` handles "name" property
- `crates/quench-runtime/src/value/function.rs` - `ValueFunction::name` field
- `crates/quench-runtime/tests/runtime_issues.rs` - regression test
