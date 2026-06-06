# Task 030: Add Per-Example HIR Runtime Unit Tests for All 89 Examples

**Priority:** P2-Medium  
**Phase:** 4 — Verification & Hardening  
**ETA:** 4–6 hours  
**Depends on:** 022, 024, 025

## The Problem

Currently, `hir_runtime.rs` has **~70 inline tests** covering only ~15 examples. The other **74 examples have zero automated verification**.

The inline tests also repeat the same pattern:

```rust
#[test]
fn test_ink_aligned() {
    let src = std::fs::read_to_string("examples/ink-aligned/tui/app.tsx").unwrap();
    let result = render_tsx(&src, 80, 24);
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("Centered"));
}
```

This pattern should be generated, not hand-written 89 times.

## Why This Matters

- EXECUTE.md: *"All the changes and complicated sections must be covered with unit-tests. High test coverage is a requirement."*
- Without per-example tests, a refactor of `eval_expr` or `apply_box_prop` can break an example silently.
- The goal is **100% matching** — we need 100% test coverage to defend it.

## Steps

### Step 1: Create a test generator macro

In `tests/interpreter_static.rs`:

```rust
macro_rules! ink_example_test {
    ($name:ident, $path:expr, $expected:expr) => {
        #[test]
        fn $name() {
            let src = std::fs::read_to_string($path)
                .unwrap_or_else(|e| panic!("failed to read {}: {}", $path, e));
            let output = render_tsx(&src, 80, 24)
                .unwrap_or_else(|e| panic!("render failed for {}: {:?}", $path, e));
            for expected in $expected {
                assert!(
                    output.contains(expected),
                    "{}: expected output to contain '{}', got:\n{}",
                    $path, expected, output
                );
            }
        }
    };
}
```

### Step 2: Generate tests for every static example

```rust
ink_example_test!(test_ink_aligned, "examples/ink-aligned/tui/app.tsx", &["Centered"]);
ink_example_test!(test_ink_border_color, "examples/ink-border-color/tui/app.tsx", &["green", "border"]);
ink_example_test!(test_ink_spacer, "examples/ink-spacer/tui/app.tsx", &["First", "Right"]);
// ... 89 tests
```

You can generate this list with a small script:

```bash
for f in examples/*/tui/app.tsx; do
    name=$(basename $(dirname $(dirname $f)))
    echo "ink_example_test!(test_${name//-/_}, \"$f\", &[]);"
done > tests/interpreter_static_generated.rs
```

Then manually fill in expected substrings for each.

### Step 3: Categorize tests

Split into modules by feature:

```
tests/
├── interpreter/
│   ├── mod.rs
│   ├── layout.rs         # Box, flex, padding, margin, align
│   ├── text.rs           # Text, color, bold, italic, etc.
│   ├── border.rs         # borderStyle, borderColor, partial borders
│   ├── hooks.rs          # useState, useEffect, useContext, useMemo
│   ├── fragments.rs      # <>...</>
│   ├── conditional.rs    # {cond && <Text>x</Text>}
│   ├── stdlib.rs         # String/array methods
│   └── static_examples.rs # One test per example file
```

### Step 4: Run all tests and fix failures

```bash
cargo test --test interpreter_static
```

For each failure:
1. If it's a missing feature → implement it (may spawn new task).
2. If it's a normalization issue → adjust the assertion.
3. If it's a genuine bug → fix it.

### Step 5: Track coverage

Add a coverage summary script:

```bash
#!/bin/bash
total=$(ls examples/*/tui/app.tsx | wc -l)
tested=$(grep -r "ink_example_test" tests/ | wc -l)
echo "Examples with tests: $tested / $total"
```

Target: 89/89.

## Acceptance Criteria

- [ ] Every example in `examples/` has at least one Rust test that runs `render_tsx` on its `tui/app.tsx`.
- [ ] Tests are in `tests/` directory, not inline in `hir_runtime.rs`.
- [ ] `cargo test` passes with all 89 examples.
- [ ] A script or `build.rs` check verifies that no example is untested.

## Notes

- For interactive examples, the test should assert that the **initial static frame** renders without panic. The exact output may differ from deno due to hook stubs — document expected differences in a comment above the test.
- If an example requires a feature not yet implemented (e.g. `measure`, `table`), mark the test `#[ignore = "requires measure support"]` and create a follow-up task.
