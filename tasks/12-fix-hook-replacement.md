# Task 12: Fix hook replacement in compiler (substring collision)

## Status: completed

## Date: 2026-06-29

## Problem

The `prefix_hooks` function in `src/compiler/mod.rs` used simple string replacement to prefix React/Ink hooks with `ink.`:

```rust
result = result.replace(hook, &format!("ink.{}", hook));
```

This caused a bug where hook names embedded in other identifiers were incorrectly replaced. For example:
- `MouseApp` contains `useApp` as a substring
- `MouseApp` was transformed to `Moink.useApp`
- This caused a parse error at position 3921..3922

## Solution

Changed `prefix_hooks` to use regex with word boundaries (`\b`) to only match complete hook names:

```rust
fn prefix_hooks(js: &str) -> String {
    let hooks = [...];
    let mut result = js.to_string();
    
    // Build regex pattern with word boundaries
    let all_hooks = hooks.join("|");
    let pattern = format!(r"\b({})\b", all_hooks);
    let re = regex::Regex::new(&pattern).unwrap();
    
    // Protect already-prefixed
    result = result.replace("ink.ink.", "ink.");
    
    // Replace hook names with word boundaries only
    result = re.replace_all(&result, "ink.$0").to_string();
    
    // Clean up any double-prefixing
    result.replace("ink.ink.", "ink.")
}
```

## Files Changed

- `Cargo.toml`: Added `regex = "1"` dependency
- `src/compiler/mod.rs`: Rewrote `prefix_hooks` to use regex with word boundaries

## Verification

- All 60+ examples now run successfully (including `mouse-app.tsx` which was failing)
- All 46 quench-runtime unit tests pass
- All 34 main crate tests pass
- All 3 parity tests pass
