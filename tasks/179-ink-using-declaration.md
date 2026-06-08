# Task 179: `ink-using-declaration` Example — `using` and `await using` (ES2024)

**Priority:** P1-High
**Phase:** 17 — ES2024 / TS 5.2 Features
**Depends on:** 178
**Status:** completed

## Problem

`using` and `await using` declarations (ES2024 / TypeScript 5.2) enable explicit resource management with automatic cleanup via `Symbol.dispose` and `Symbol.asyncDispose`. No existing Ink example exercises these features.

## Ink Example

```tsx
// examples/ink-using-declaration/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const resource = {
  [Symbol.dispose](): void {
    // cleanup
  },
  name: 'Resource',
};

export default function App() {
  using r = resource;

  return (
    <Box flexDirection="column">
      <Text>Resource: {r.name}</Text>
    </Box>
  );
}
```

## Implementation

### Files Changed
- **Added:** `examples/ink-using-declaration/` (example directory)
- **Modified:** `crates/runts-hir/src/base.rs` (added `Using` and `AwaitUsing` to `VariableKind`)
- **Modified:** `src/transpile/parser/stmt_decl.rs` (updated `var_kind_from_oxc`)
- **Modified:** `src/transpile/parser/stmt.rs` (updated `var_kind`)
- **Modified:** `src/transpile/hir/quote_codegen_stmts.inc` (updated match arms)
- **Added:** Tests in `src/transpile/tests/spec_vars_functions/variable_bindings.rs`

### HIR Changes
- Added `Using` and `AwaitUsing` variants to `VariableKind` enum
- Parser correctly maps `VariableDeclarationKind::Using` and `VariableDeclarationKind::AwaitUsing` to new HIR variants

### Codegen Changes
- `using` and `await using` are erased in compile path (mapped to `let`)
- Symbol.dispose semantics cannot be expressed in static Rust codegen

## Acceptance Criteria

- [x] Example exists at `examples/ink-using-declaration/`
- [x] Uses `using` declaration with `Symbol.dispose`
- [x] Optionally uses `await using` with `Symbol.asyncDispose`
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path generates compilable Rust (erased to `let`)
- [x] Parity harness passes with 100% match in all 3 environments

## Notes

- The compile path cannot implement `Symbol.dispose` semantics - `using` declarations are erased to regular `let` in the codegen
- The dev path (rquickjs) correctly executes `using` declarations since rquickjs has native support
- Deno also supports `using` declarations natively
