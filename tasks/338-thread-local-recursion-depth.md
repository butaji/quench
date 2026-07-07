> **Immediate fix for false stack-overflow errors in parallel tests.**

# Task 338: Make recursion depth counter thread-local

## Problem

`CURRENT_DEPTH` in `crates/quench-runtime/src/interpreter.rs` is a global `static AtomicUsize`. When `cargo test` runs tests in parallel, threads share the counter and simple calls like `Math.log10(1)` or `clearTimeout(1)` exceed the limit.

## Exact implementation

Edit only `crates/quench-runtime/src/interpreter.rs`:

1. Keep the global override for the max depth limit:
   ```rust
   static MAX_RECURSION_DEPTH_OVERRIDE: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_RECURSION_DEPTH);
   ```
2. Remove the global `CURRENT_DEPTH` static and add a thread-local cell:
   ```rust
   thread_local! {
       static CURRENT_DEPTH: Cell<usize> = const { Cell::new(0) };
   }
   ```
3. Rewrite `check_depth()`:
   ```rust
   fn check_depth() -> Result<(), JsError> {
       CURRENT_DEPTH.with(|cell| {
           let depth = cell.get();
           if depth >= get_max_depth() {
               Err(JsError("Maximum call stack size exceeded".to_string()))
           } else {
               cell.set(depth + 1);
               Ok(())
           }
       })
   }
   ```
4. Rewrite `release_depth()`:
   ```rust
   fn release_depth() {
       CURRENT_DEPTH.with(|cell| {
           let d = cell.get();
           if d > 0 { cell.set(d - 1); }
       });
   }
   ```
5. Rewrite `reset_depth()`:
   ```rust
   pub fn reset_depth() {
       CURRENT_DEPTH.with(|cell| cell.set(0));
   }
   ```
6. Ensure the `AtomicUsize`/`Ordering` imports remain.

## Verification

```bash
cargo test -p quench-runtime --test runtime_issues
```

Expected: 44 passed, 0 failed (parallel execution).

## Targets

- **Suite:** `runtime`
- **Batch:** 0
- **Target subset:** n/a (infrastructure)
- **Blocked by:** none
- **Exit criteria:** `runtime_issues.rs` passes in parallel with no `Maximum call stack size exceeded` errors.
