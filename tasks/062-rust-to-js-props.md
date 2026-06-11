# Task 062: Rust → Ink Props Propagation

## Status
✅ **Done**


## Goal
Provide a mechanism for developers to pass configuration, computed values, and platform info from Rust into the Ink/JS runtime.

## Problem
Currently, data only flows one way: JS creates elements → Rust renders them. There's no way for Rust to inject data (theme, locale, OS info, computed values) into JS components.

## Use Cases

### 1. CLI-driven theming
```bash
quench --prop theme=dark --prop locale=en-US examples/app.js
```

### 2. Rust-computed values in JS
```js
const { platform } = useBridge().config;
// platform.os === "macos", platform.arch === "aarch64"
```

### 3. Terminal capability detection
```js
const { terminal } = useBridge().config;
// terminal.colorSupport === 24 (truecolor)
// terminal.hasMouse === true
```

## Proposed Architecture

### Rust Side (`src/bridge_config.rs`)

```rust
//! Bridge Configuration — Propagate props from Rust to Ink/JS

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct BridgeConfig {
    props: HashMap<String, String>,      // User-defined --prop key=value
    platform: PlatformInfo,              // OS, arch, version
    terminal: TerminalInfo,              // Color support, mouse, unicode
}

#[derive(Debug, Clone, Default)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub version: String,
}

#[derive(Debug, Clone, Default)]
pub struct TerminalInfo {
    pub color_support: u8,   // 0=none, 8=256, 16=high, 24=truecolor
    pub has_mouse: bool,
    pub has_unicode: bool,
}

impl BridgeConfig {
    pub fn new() -> Self { /* detect platform/terminal */ }
    pub fn prop(self, key: &str, value: &str) -> Self;
    pub fn from_args(args: &[String]) -> Self;  // Parse --prop flags
    pub fn to_js_injection(&self) -> String;    // JS snippet for VM
}
```

### JS Side (`src/runtime.js`)

```js
// 14. Bridge Config — Rust-injected values
function useBridge() {
  return useMemo(() => {
    const tb = globalThis.__quench || { config: {} };
    return {
      config: tb.config || {},
      // Helper for typed access
      get: (key, defaultValue) => tb.config[key] ?? defaultValue,
    };
  }, []);
}

// Export
globalThis.useBridge = useBridge;
```

### Integration (`src/main.rs`)

```rust
// After creating QuickJS context, before loading user code:
let config = BridgeConfig::from_args(&args);
ctx.with(|ctx| {
    ctx.eval(config.to_js_injection())
        .expect("Failed to inject bridge config");
});
```

### CLI Integration

```rust
// Parse --prop flags
if args.contains(&"--prop".to_string()) {
    println!("  --prop KEY=VALUE  Pass a prop to the JS runtime");
}
```

## Data Flow

```
CLI: quench --prop theme=dark examples/app.js
                │
                ▼
Rust: BridgeConfig::from_args()
      ├── props: { theme: "dark" }
      ├── platform: { os: "macos", arch: "aarch64", version: "0.1.0" }
      └── terminal: { colorSupport: 24, hasMouse: true, hasUnicode: true }
                │
                ▼
Rust: config.to_js_injection()
      → "globalThis.__quench = { config: { theme: 'dark', ... } };"
                │
                ▼
QuickJS: ctx.eval(injection_snippet)
                │
                ▼
JS: const { theme } = useBridge().config;  // "dark"
    const { os } = useBridge().config.platform;  // "macos"
```

## Files to Create/Modify

### New Files
- `src/bridge_config.rs` — Config struct, platform detection, CLI parsing

### Modified Files
- `src/main.rs` — Parse `--prop` flags, inject config before user code
- `src/runtime.js` — Add `useBridge()` hook, export to `globalThis`
- `src/ink_js.rs` — Register `useBridge` in rquickjs globals (optional)

## Acceptance Criteria
- [ ] `--prop KEY=VALUE` CLI flag works
- [ ] `useBridge().config` accessible from JS
- [ ] `useBridge().config.platform` has OS/arch/version
- [ ] `useBridge().config.terminal` has color/mouse/unicode
- [ ] Multiple `--prop` flags accumulate
- [ ] No props = empty config (no errors)
- [ ] Zero performance impact when not used

## Dependencies
- Task 060 (compatibility validation — same pattern)

## SPEC Reference
§2 Stack (Bridge layer)
