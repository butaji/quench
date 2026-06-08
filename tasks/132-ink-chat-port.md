# Task 132: Port `../tui1` Chat Example — Real-World Feature Audit

**Priority:** P0-Critical
**Phase:** 12 — Real-World Validation
**Depends on:** 078

## Goal

The `../tui1/mod.tsx` example is a real-world Ink chat UI. It must compile and render identically in all 3 environments (deno, `runts dev`, `runts build`) with 100% output match. This task serves as a comprehensive audit of what features are still missing.

## Source

```tsx
/** @jsxImportSource react */
import React, { useState, useEffect, useRef } from "react";
import { render, Text, Box } from "ink";

interface Message { id: number; role: "user" | "claude"; content: string; timestamp: string; }

const C = { bg: "#0c0c0c", fg: "#4a4a4a", fgMid: "#6a6a6a", fgBright: "#909090",
  accent: "#8b7cf4", success: "#3ebd6a", warning: "#eab84a", dim: "#282828" };

const App: React.FC = () => { ... };
render(React.createElement(App));
```

Full source: `../tui1/mod.tsx` (copied to `examples/ink-chat/tui/app.tsx`)

---

## Feature Breakdown — What's Used vs What's Covered

### ✅ Already Covered (Phase 6–7 tasks)

| Feature | Used? | Task | Status |
|---------|-------|------|--------|
| `useState` | ✅ | 035/036/042 | completed |
| `useEffect` | ✅ | 035/036 | completed |
| `useRef` | ✅ | 035/036 | completed |
| `interface` | ✅ | 082 (Phase 10) | pending |
| Object literals | ✅ | 046/048 | completed |
| Arrow functions | ✅ | 054 | completed |
| Generic type params `useState<Message[]>` | ✅ | 067 | completed |
| `typeof` operator | ✅ | 050 | completed |
| Optional chaining `?.` | ✅ | 049/068 | completed |
| Array spread `[...prev, x]` | ✅ | 046 | completed |
| `switch` / `case` / `break` | ✅ | 042 | completed |
| Ternary `?:` | ✅ | 061 | completed |
| Nested ternary | ✅ | 061 | completed |
| `String.prototype.slice` | ✅ | implicit | completed |
| `String.prototype.trim` | ✅ | implicit | completed |
| `Array.prototype.map` | ✅ | implicit | completed |
| `Array.prototype.pop` | ✅ | implicit | completed |
| `Array.prototype.join` | ✅ | implicit | completed |
| `Array.prototype.forEach` | ✅ | implicit | completed |
| `Object.keys` | ✅ | implicit | completed |
| Logical ops `&&` `\|\|` | ✅ | 051 | completed |
| `Math.floor` / `Math.random` | ✅ | implicit | completed |
| JSX elements, attrs, children | ✅ | 061 | completed |
| JSX expression blocks `{}` | ✅ | 061 | completed |
| `const` type annotation `const App: React.FC` | — | 082 (Phase 10) | pending |

### ❌ NOT Covered — Missing Tasks

These features are used in `../tui1` but have **no task coverage**:

| Feature | Location in Source | Why It Fails | Needed Task |
|---------|-------------------|--------------|-------------|
| **`process` global** | `process.on`, `process.exit`, `process.stdin`, `process.stdout` | rquickjs has no `process` global | **NEW: Task 133** |
| **`setInterval` / `clearInterval`** | Timer for elapsed time display | No shim in React runtime | **NEW: Task 134** |
| **`Date` object** | `new Date()`, `toLocaleTimeString(...)` | `Date` may not exist in rquickjs | **NEW: Task 135** |
| **`Array.prototype.splice`** | `inputBuffer.splice(pos, 0, _str)` | Runtime method | **NEW: Task 136** |
| **React Fragment shorthand `<>`** | `<>...</>` in `renderInput` | JSX transform may not handle `<> | **NEW: Task 137** |
| **`import("node:readline")`** | Dynamic import of node built-in | rquickjs cannot load node modules | **NEW: Task 138** |
| **`/** @jsxImportSource react */`** | Top of file | oxc transform may strip or error | **NEW: Task 139** |
| **`render()` call in module** | `render(React.createElement(App))` | Double-render conflict with main.tsx | **NEW: Task 140** |

### ⚠️ Partially Covered — May Need Extension

| Feature | Task | Gap |
|---------|------|-----|
| Dynamic import `import()` | 073 (completed) | Only tested with user modules, not node built-ins |
| `React.FC` type | 082 (pending) | Type alias — may or may not be erased cleanly |
| `Number.prototype.toFixed` | implicit | Need to verify in rquickjs bridge |

---

## Current Error

```
Error: QuickJS error: Bundle eval failed: Exception
```

Root cause: **Unknown** — needs detailed error extraction from rquickjs context.
Most likely candidates (in order):

1. `process` is `undefined` — `process.on("SIGINT", ...)` throws immediately
2. `import("node:readline")` — dynamic import fails in rquickjs
3. JSX pragma `/** @jsxImportSource react */` — conflicts with transform
4. `Date` constructor missing in rquickjs

---

## Acceptance Criteria

- [ ] All features from the "Missing Tasks" table above have dedicated tasks
- [ ] Example renders in `runts dev --once` without QuickJS exception
- [ ] Output matches deno reference 100% (after `--once` normalization)
- [ ] Compile path generates compilable Rust (may need codegen fixes)
- [ ] `cargo build` passes with 0 errors, 0 warnings
