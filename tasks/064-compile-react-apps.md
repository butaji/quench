# Task 064: Compile React+Ink Apps for TuiBridge

## Goal
Transform React+Ink source files (`mod.tsx`, `app.ts`, etc.) into TuiBridge-compatible JS using a lightweight TSX compiler.

## Current Status

| Component | Status | Notes |
|-----------|--------|-------|
| `src/compiler/mod.rs` | ✅ | Regex-based JSX compilation |
| `src/compiler/shim.rs` | ✅ | React/Ink import mappings |
| `compile_tsx()` | ✅ | Main entry point |
| `compile_file()` | ✅ | Compile file from disk |
| Import removal | ✅ | Removes react/ink imports |
| Self-closing tags | ✅ | `<Tag />` → `ink.createElement()` |
| Attributes | ✅ | Parses props into JS object |
| CLI integration | ✅ | `--compile` and `--run` flags |

### Limitations

- Nested JSX transformation is limited (regex-based)
- For full TSX support, pre-compile with esbuild or use JS examples

## Implementation

### Architecture (Regex-based)

```
Input: mod.tsx
    │
    ▼
┌─────────────────────────────────────┐
│  Import Removal                     │
│  - Remove react/ink imports         │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  Regex JSX Transform                │
│  - Self-closing tags: <Tag/>        │
│  - Attribute parsing                │
└─────────────────────────────────────┘
    │
    ▼
Output: mod-tb.js (TuiBridge-compatible)
```

### Rust Module Structure

```
src/
├── main.rs              # Entry point
└── compiler/
    ├── mod.rs           # Public API: compile_tsx(), compile_file()
    └── shim.rs          # Import mappings, polyfills
```

### CLI Integration

```bash
# Compile TSX → TuiBridge JS
tuibridge --compile mod.tsx -o mod-tb.js

# Run directly (compile + execute)
tuibridge --run mod.tsx
```

## Features

### ✅ Implemented

1. **Import Removal**
   - Removes `import { useState } from "react"`
   - Removes `import { render, Box } from "ink"`
   - Handles default and named imports

2. **JSX Transformation**
   - `<Box />` → `ink.createElement("Box", null)`
   - `<Box attr="val" />` → `ink.createElement("Box", {attr: "val"})`
   - `<Box attr={val} />` → `ink.createElement("Box", {attr: val})`
   - Boolean attributes work

### ⚠️ Limitations

- Nested JSX (`<Box><Text>Hi</Text></Box>`) not fully transformed
- For nested JSX, use JS examples or pre-compile with esbuild

## Usage Notes

The examples in this project are written in JS (`.js` files) which run directly in TuiBridge. The TSX compiler is an optional feature for users who want to write in TSX.

For production use:
```bash
# Option 1: Use JS examples directly
tuibridge examples/counter.js

# Option 2: Pre-compile TSX with esbuild
esbuild app.tsx --outfile=app.js --bundle
tuibridge app.js

# Option 3: Use the built-in compiler (limited)
tuibridge --compile app.tsx -o app.js
tuibridge app.js
```

## Acceptance Criteria

- [x] `tuibridge::compiler::compile_tsx()` exists
- [x] `tuibridge::compiler::compile_file()` exists
- [x] CLI `--compile` flag works
- [x] CLI `--run` flag works
- [x] Import removal works
- [x] Self-closing JSX tags transformed
- [x] Attributes parsed correctly
- [ ] Nested JSX fully transformed (deferred)

## Files Created/Modified

### New Files
- `src/compiler/mod.rs` — Regex-based TSX compiler
- `src/compiler/shim.rs` — Import mappings

### Modified Files
- `src/main.rs` — Added `--compile` and `--run` CLI flags
- `Cargo.toml` — Added regex dependency for compiler feature

## Dependencies
- Task 001-058 (core functionality)

## SPEC Reference
§2 Stack (Build/Compile layer)
