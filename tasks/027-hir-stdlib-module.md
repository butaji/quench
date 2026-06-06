# Task 027: Extract String/Array Stdlib Polyfills into a Reusable HIR Stdlib Module

**Priority:** P1-High  
**Phase:** 2 — HIR Runtime Core Engine  
**ETA:** 3 hours  
**Depends on:** 022

## The Problem

`call_string_method` and `call_array_method` are embedded in `hir_runtime.rs` and together comprise **~400 lines** of near-identical callback invocation logic.

Every array method (`map`, `filter`, `reduce`, `forEach`, `find`, `some`, `every`, `includes`, `indexOf`, `join`, `slice`) repeats:

```rust
let callback = arguments.first().map(|a| self.eval_expr(a)).transpose()?.unwrap_or(Value::Undefined);
if let Value::Function { params, body } = callback {
    let saved_scope = self.scope.clone();
    self.scope.insert(params.get(0).cloned().unwrap_or_default(), item.clone());
    if let Some(idx_param) = params.get(1) {
        self.scope.insert(idx_param.clone(), Value::Number(i as f64));
    }
    let result = self.call_function(&params, &body, &[])?;
    self.scope = saved_scope;
    // ... method-specific logic
}
```

This pattern is repeated **9 times**. Any bug in callback invocation (e.g. not passing the array as `this`, not handling sparse arrays, etc.) exists in 9 places.

## Why This Matters

- DRY violation at scale.
- Adding a new array method (e.g. `flatMap`, `at`, `toSorted`) requires copy-pasting 20 lines.
- The callback invocation logic is subtle (scope save/restore, param binding). It should be written once.

## Steps

### Step 1: Create `src/interpreter/stdlib.rs`

Move `call_string_method` and `call_array_method` here.

### Step 2: Extract a callback invoker helper

```rust
impl Interpreter {
    fn invoke_callback(
        &mut self,
        callback: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        match callback {
            Value::Function { params, body } => {
                let saved_scope = self.scope.clone();
                for (i, param) in params.iter().enumerate() {
                    let val = args.get(i).cloned().unwrap_or(Value::Undefined);
                    self.scope.insert(param.clone(), val);
                }
                let result = self.eval_expr(body);
                self.scope = saved_scope;
                result
            }
            _ => Ok(Value::Undefined),
        }
    }
}
```

### Step 3: Rewrite array methods using the helper

```rust
fn call_array_method(
    &mut self,
    arr: Vec<Value>,
    method_name: &str,
    arguments: &[Expr],
) -> Result<Value, RuntimeError> {
    match method_name {
        "map" => {
            let callback = self.eval_expr(&arguments[0])?;
            let results: Vec<Value> = arr
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    self.invoke_callback(&callback, &[item.clone(), Value::Number(i as f64)])
                })
                .collect::<Result<_, _>>()?;
            Ok(Value::Array(results))
        }
        "filter" => {
            let callback = self.eval_expr(&arguments[0])?;
            let mut results = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                let keep = self.invoke_callback(&callback, &[item.clone(), Value::Number(i as f64)])?;
                if let Value::Boolean(true) = keep {
                    results.push(item.clone());
                }
            }
            Ok(Value::Array(results))
        }
        // ... etc
        _ => Ok(Value::Undefined),
    }
}
```

### Step 4: Add missing methods

Add these JS array methods that Ink examples may use:
- `flatMap`
- `at` (ES2022)
- `toSorted`, `toReversed`, `toSpliced` (if needed)
- `fill`
- `findIndex`
- `lastIndexOf`

Add these string methods:
- `replaceAll`
- `match` (basic regex-free version)
- `padStart` / `padEnd` (already present — verify)
- `repeat` (already present — verify)

### Step 5: Unit tests for every method

Create `tests/interpreter_stdlib.rs`:

```rust
#[test]
fn test_array_map() {
    let src = r#"
export default function App() {
  const items = ["a", "b", "c"];
  const upper = items.map(x => x.toUpperCase());
  return <Text>{upper.join("-")}</Text>;
}
"#;
    assert!(render_tsx(src, 80, 24).unwrap().contains("A-B-C"));
}

#[test]
fn test_array_filter() {
    let src = r#"
export default function App() {
  const nums = [1, 2, 3, 4, 5];
  const evens = nums.filter(n => n % 2 === 0);
  return <Text>{evens.join(",")}</Text>;
}
"#;
    assert!(render_tsx(src, 80, 24).unwrap().contains("2,4"));
}

#[test]
fn test_string_replace_all() {
    let src = r#"
export default function App() {
  const s = "foo-bar-baz";
  return <Text>{s.replaceAll("-", "_")}</Text>;
}
"#;
    assert!(render_tsx(src, 80, 24).unwrap().contains("foo_bar_baz"));
}
```

## Acceptance Criteria

- [ ] `call_string_method` and `call_array_method` live in `interpreter/stdlib.rs`.
- [ ] Callback invocation logic is in exactly one place: `invoke_callback`.
- [ ] Every existing array/string method has a unit test.
- [ ] New methods (`flatMap`, `at`, `replaceAll`) are implemented and tested.
- [ ] All 89 examples still pass.

## Notes

- Do not implement full ECMAScript semantics. Implement exactly what the 89 Ink examples need.
- `this` binding is not required — Ink callbacks are always arrow functions or explicit.
