# Task 029: Make runts Compile Produce Working Binaries for Every Static Example

**Priority:** P1-High  
**Phase:** 3 — Compile Path  
**ETA:** 4–6 hours  
**Depends on:** 028

## The Problem

`runts build --release` is the **third environment** in EXECUTE.md:

> 3) runts compile (ts/tsx transpile to rust in-memory, and then compile)

Currently:
- The compile path has **zero integration tests** in the Rust test suite.
- The parity scripts try to exercise it, but it's unclear if any example actually produces a runnable binary.
- The plugin-based ephemeral build uses `TempDir`, generates a `Cargo.toml`, runs `cargo build`, and copies the binary. Many things can go wrong:
  1. Plugin codegen emits invalid Rust.
  2. Generated `Cargo.toml` has wrong dependency paths.
  3. `runts-lib` path resolution fails.
  4. The binary runs but panics.

## Why This Matters

- EXECUTE.md demands 100% parity across **all 3 environments**.
- If `runts compile` doesn't work, the project is a dev-only toy.
- Users expect `runts build --release` to produce a working static binary.

## Steps

### Step 1: Verify the compile path on one static example

Pick the simplest example: `examples/ink-hello-world` or `examples/ink-text-props`.

```bash
cd examples/ink-text-props
../../../target/debug/runts build --release --plugin ratatui
```

Expected: binary appears at `./target/release/runts-app` and prints "HIGHLIGHTED" when run.

If this fails, fix the first error and repeat.

### Step 2: Fix `runts-lib` path resolution

`src/commands/build/mod.rs` has `find_runts_lib_path()` which tries:
1. Relative to current exe
2. Ancestors of exe dir
3. Project root
4. `CARGO_MANIFEST_DIR`

This is fragile. Replace with:

```rust
fn find_runts_lib_path(_project_root: &Path) -> PathBuf {
    // At build time, embed the path via env!
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crates")
        .join("runts-lib")
}
```

This works because `runts` is always built from its own workspace root.

### Step 3: Ensure plugin codegen generates a valid `main.rs`

For a single-file TUI example, the generated `main.rs` should look like:

```rust
fn main() {
    let vnode = app();
    let options = runts_ink::RenderOptions::default();
    runts_ink::render_to_string(vnode, options).unwrap();
}
```

Or for interactive examples:

```rust
fn main() {
    runts_ink::render(Box::new(|props| app()), runts_ink::Props::default(), runts_ink::RenderOptions::default()).unwrap();
}
```

Verify `runts-ratatui/src/plugin.rs` generates this shape.

### Step 4: Add integration tests for the compile path

Create `tests/compile_path.rs`:

```rust
use std::process::Command;

#[test]
fn test_compile_ink_text_props() {
    let output = Command::new("cargo")
        .args(&["run", "--", "build", "--release", "--plugin", "ratatui"])
        .current_dir("examples/ink-text-props")
        .output()
        .expect("cargo run failed");

    assert!(output.status.success(), "build failed: {}", String::from_utf8_lossy(&output.stderr));

    let binary = std::path::Path::new("examples/ink-text-props/target/release/runts-app");
    assert!(binary.exists(), "binary not found");

    let run_output = Command::new(binary)
        .output()
        .expect("failed to run binary");

    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("HIGHLIGHTED"), "unexpected output: {}", stdout);
}
```

Add one test per example category:
- Static text (`ink-text-props`)
- Layout (`ink-box`, `ink-aligned`)
- Borders (`ink-bordered`, `ink-border-color`)
- Hooks (`ink-counter` — if interactive path works)

### Step 5: Run compile tests for all static examples

Use the parity harness:

```bash
./scripts/parity.sh --env compile --examples ink-*
```

For each example:
- Build must succeed.
- Binary must exit 0.
- stdout must match deno/HIR output (within normalization rules).

### Step 6: Document compile-path limitations

Some features may never compile (e.g. `useInput` event loop without a real TTY). Document:

```markdown
## Compile Path Known Limitations
- Interactive examples require a terminal. CI runs must use `script` or `screen`.
- `useEffect` with async fetch is not yet supported in codegen.
- Dynamic imports are not supported.
```

## Acceptance Criteria

- [ ] `runts build --release --plugin ratatui` works on `examples/ink-text-props`.
- [ ] Generated binary exits 0 and prints expected text.
- [ ] Integration test exists for at least 5 static examples.
- [ ] `./scripts/parity.sh --env compile` runs all 89 examples and reports results.
- [ ] `find_runts_lib_path` is deterministic (uses `CARGO_MANIFEST_DIR`).

## Notes

- Start with static examples only. Interactive examples are Phase 4.
- If a plugin codegen bug is found, fix it in the plugin crate, not by working around it in the build command.
- Use `RUNTS_KEEP_BUILD=1` to inspect generated source when debugging.
