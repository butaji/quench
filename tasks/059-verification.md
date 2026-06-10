# Task 059: Verification & Testing

## Goal
Verify the implementation meets all acceptance criteria and add test coverage.

## Current Status

| Criteria | Status | Notes |
|----------|--------|-------|
| Build succeeds | ✅ | `cargo build --release` passes |
| Binary size < 5 MB | ✅ | **2.1 MB** |
| FFI tests | ✅ | `examples/simple.js` passes |
| clippy | ✅ | Warnings only |
| Test suite | ⚠️ | **No test files exist** |
| Parity harness | ⚠️ | Needs PTY for TTY emulation |
| Hot reload benchmark | ⏳ | Not benchmarked |

## Remaining Work

### 1. Test Suite

Add `#[cfg(test)]` modules and/or `tests/` integration tests:

```rust
// src/bridge.rs
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_text_measurement() {
        let result = measure_text_internal("Hello", 80);
        assert!(result.width > 0);
        assert_eq!(result.height, 1);
    }
    
    #[test]
    fn test_color_parsing() {
        assert_eq!(parse_color("red"), Some(Color::Red));
        assert_eq!(parse_color("#ff0000"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("#f00"), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(parse_color("invalid"), None);
    }
    
    #[test]
    fn test_keycode_mapping() {
        assert_eq!(keycode_to_ink_name(&KeyCode::Char('q')), "q");
        assert_eq!(keycode_to_ink_name(&KeyCode::Up), "upArrow");
        assert_eq!(keycode_to_ink_name(&KeyCode::F(1)), "f1");
    }
}
```

### 2. PTY for Parity Harness

The current `scripts/parity.sh` doesn't handle TTY emulation properly. Fix with:

```bash
# Use script(1) or a PTY library for proper terminal emulation
# Example using script command:
script -q -c "tuibridge examples/counter.js" /dev/null | head -100 > output.tui
```

Or integrate a PTY crate like `portable-pty` or `pty-process` into the harness.

### 3. Hot Reload Benchmark

Add a benchmark to measure hot reload latency:

```rust
#[cfg(feature = "hotreload")]
#[test]
fn test_hot_reload_latency() {
    use std::time::Instant;
    
    let start = Instant::now();
    // Simulate hot reload cycle:
    // 1. Detect file change
    // 2. Read new file
    // 3. Eval in existing VM
    // 4. Trigger remount
    let elapsed = start.elapsed();
    
    assert!(elapsed.as_millis() < 50, "Hot reload took {}ms", elapsed.as_millis());
}
```

### 4. Terminal Output Verification

Create a script to verify visual output in a real TTY:

```bash
#!/bin/bash
# Verify output in actual terminal
osascript -e 'tell app "Terminal" to do script "tuibridge examples/counter.js"'
# Then manually verify the output looks correct
```

## Acceptance Criteria

- [ ] `cargo test` runs and passes with meaningful tests
- [ ] `cargo test --all-features` passes (includes hotreload)
- [ ] Parity harness captures TTY output correctly
- [ ] Hot reload < 50 ms benchmarked and passing
- [ ] Examples verified visually in actual terminal

## Dependencies
- Task 001–057 (all prior tasks complete)

## SPEC Reference
§5 Testing Strategy, §6 Performance Targets
