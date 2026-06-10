# Task 063: React Reconciler Bridge

## Status
📅 **Deferred** — Approach A: React host config. Approach B: Build-time swap with @tuibridge/ink-shim.


## Goal
Enable running full React apps (bundled with `react-reconciler`) under TuiBridge by providing a bridge host config.

## Problem

The `../tui1` app bundles React + Ink's reconciler into a 28,000-line ESM bundle. Running it under TuiBridge fails because:

1. **Node.js imports** — Bundle starts with `import { createRequire } from "node:module"`; QuickJS lacks Node.js built-ins
2. **React reconciler** — Ink's reconciler uses `react-reconciler` which expects a full host environment
3. **Double reconciler** — TuiBridge already has a reconciler in `runtime.js`; React apps bring their own
4. **Exit without render** — The bundled app never calls TuiBridge's `render()`, so no tree is built

## Root Cause

```
tui1/dist/bundle.js (28,479 lines)
├── React core (useState, useEffect, etc.)
├── react-reconciler (React's tree diff engine)
├── Ink's host config (maps React elements → Ink nodes)
└── Node.js ESM shim (createRequire, __filename, __dirname)

TuiBridge runtime.js (~1,070 lines)
├── Custom reconciler (NOT React's)
├── Bridge wrappers (__ink_create_node → Rust FFI)
└── No Node.js module system
```

React apps expect to use React's reconciler. TuiBridge provides its own. These cannot coexist.

## Solution Approaches

### Approach A: React Host Config for TuiBridge (Recommended)

Implement a proper `react-reconciler` host config that bridges React's reconciler to TuiBridge's Rust tree.

**Architecture:**

```
User App (React + JSX)
    │
    │ import { render } from "@tuibridge/react-renderer"
    ▼
react-reconciler (Facebook's reconciler)
    │
    │ Host Config API:
    │   createInstance(type, props) → __ink_create_node()
    │   appendChild(parent, child)  → __ink_append_child()
    │   commitUpdate(...)           → __ink_commit_update()
    ▼
TuiBridge Host Config (JS)
    │
    │ __ink_call FFI
    ▼
Rust Bridge (bridge.rs)
    │
    ▼
Ink Runtime + Yoga + ratatui
```

**Files:**
- `src/react_host_config.rs` — Rust-side support (if needed)
- `src/runtime_react.js` — React reconciler host config (loaded optionally)

**API:**
```js
// New module: @tuibridge/react-renderer
import { createRenderer } from '@tuibridge/react-renderer';

const { render } = createRenderer({
  // TuiBridge FFI is already available as globals
  createNode: __ink_create_node,
  appendChild: __ink_append_child,
  // ...
});

render(<App />);
```

**Challenges:**
- `react-reconciler` is a CJS package; QuickJS can't load npm packages natively
- Would need to bundle `react-reconciler` into `runtime_react.js` (adds ~50KB minified)
- React's reconciler may use Node.js APIs internally

### Approach B: Build-Time Target Swap (Alternative)

Replace Ink imports with TuiBridge API at build time using esbuild aliases.

**User build config:**
```js
// esbuild.config.js
await esbuild.build({
  entryPoints: ['app.tsx'],
  bundle: true,
  alias: {
    'ink': '@tuibridge/ink-shim',  // Replace Ink with TuiBridge shim
    'react': '@tuibridge/react-shim', // Optional: replace React hooks
  },
});
```

**`@tuibridge/ink-shim` package:**
```js
// Provides the same exports as Ink, but uses TuiBridge globals
export const render = globalThis.ink.render;
export const Box = globalThis.ink.Box;
export const Text = globalThis.ink.Text;
export const useInput = globalThis.ink.useInput;
// ...etc
```

**Pros:**
- No reconciler duplication
- Zero runtime overhead
- Apps compile to TuiBridge-native code

**Cons:**
- Requires recompilation of the app
- Not drop-in for existing bundles

### Approach C: Full Node.js Polyfill (Not Recommended)

Polyfill enough of Node.js to run bundled React apps directly.

**Would need:**
- `node:module` — `createRequire`, `Module` class
- `node:path` — `join`, `resolve`
- `node:url` — `fileURLToPath`
- `node:process` — `stdout`, `stderr`, `env`, `exit`
- `node:fs` — File operations (React DevTools may use this)

**Size impact:** ~100KB+ of polyfills
**Complexity:** Extremely high — React's internals are deeply tied to Node.js

## Recommended Path: Approach A + B

### Phase 1: Build-Time Target (Approach B)

Create `@tuibridge/ink-shim` npm package that maps Ink's API to TuiBridge globals.

```bash
# User rebuilds their app targeting TuiBridge
npm install @tuibridge/ink-shim
npx esbuild app.tsx \
  --bundle \
  --alias:ink=@tuibridge/ink-shim \
  --outfile=dist/tuibridge-app.js

# Run
tuibridge dist/tuibridge-app.js
```

**`@tuibridge/ink-shim` exports:**
```typescript
export const render: typeof import('ink').render;
export const Box: typeof import('ink').Box;
export const Text: typeof import('ink').Text;
export const Static: typeof import('ink').Static;
export const Newline: typeof import('ink').Newline;
export const Spacer: typeof import('ink').Spacer;
export const useInput: typeof import('ink').useInput;
export const useApp: typeof import('ink').useApp;
export const useStdin: typeof import('ink').useStdin;
export const useStdout: typeof import('ink').useStdout;
export const useStderr: typeof import('ink').useStderr;
export const useFocus: typeof import('ink').useFocus;
export const useFocusManager: typeof import('ink').useFocusManager;
export const measureElement: typeof import('ink').measureElement;
```

### Phase 2: React Reconciler Bridge (Approach A)

For apps that MUST use React's reconciler (e.g., complex third-party React components):

```js
// runtime_react.js — optional, loaded when --react flag is used
const ReactReconciler = require('react-reconciler'); // Bundled inline

const hostConfig = {
  supportsMutation: true,
  supportsPersistence: false,
  
  createInstance(type, props, rootContainer, hostContext, internalHandle) {
    const tag = 'ink-' + type.toLowerCase();
    const nodeId = __ink_create_node(tag, JSON.stringify(props));
    return { nodeId, tag };
  },
  
  createTextInstance(text, rootContainer, hostContext, internalHandle) {
    const nodeId = __ink_create_text_node(text);
    return { nodeId, text };
  },
  
  appendInitialChild(parent, child) {
    __ink_append_child(parent.nodeId, child.nodeId);
  },
  
  appendChild(parent, child) {
    __ink_append_child(parent.nodeId, child.nodeId);
  },
  
  removeChild(parent, child) {
    __ink_remove_child(parent.nodeId, child.nodeId);
  },
  
  insertBefore(parent, child, beforeChild) {
    __ink_insert_before(parent.nodeId, child.nodeId, beforeChild.nodeId);
  },
  
  finalizeInitialChildren() {
    return false;
  },
  
  prepareUpdate(instance, type, oldProps, newProps, rootContainer, hostContext) {
    // Return payload if props changed
    return JSON.stringify(newProps) !== JSON.stringify(oldProps) ? newProps : null;
  },
  
  commitUpdate(instance, updatePayload, type, oldProps, newProps, internalHandle) {
    __ink_commit_update(instance.nodeId, JSON.stringify(updatePayload));
  },
  
  commitTextUpdate(textInstance, oldText, newText) {
    __ink_set_text(textInstance.nodeId, newText);
  },
  
  // ... other host config methods
};

const reconciler = ReactReconciler(hostConfig);

export function render(element) {
  const container = { nodeId: __ink_create_root() };
  const root = reconciler.createContainer(container, 0, null, false, null, '', () => {}, null);
  reconciler.updateContainer(element, root, null, () => {
    __ink_commit();
  });
}
```

**Usage:**
```bash
# Load React reconciler bridge
tuibridge --react ../tui1/dist/bundle.js
```

**Size:** `runtime_react.js` would be ~100KB (react-reconciler + host config)

## Comparison

| Approach | Drop-in | Bundle Size | Complexity | Performance |
|----------|---------|-------------|------------|-------------|
| A: React Host Config | ✅ Yes | +100KB | High | Same |
| B: Build-Time Swap | ❌ Rebuild | Same | Low | Better (no double reconciler) |
| C: Node.js Polyfill | ✅ Yes | +100KB | Very High | Slower |

## Files to Create

### New Files
- `packages/ink-shim/` — npm package: `@tuibridge/ink-shim`
  - `index.js` — TuiBridge-compatible Ink API
  - `index.d.ts` — TypeScript definitions
- `src/runtime_react.js` — React reconciler host config (optional runtime)

### Modified Files
- `src/main.rs` — Add `--react` flag to load React bridge
- `docs/SPEC.md` — Document React bridge usage

## Acceptance Criteria

### Phase 1 (Build-Time Swap)
- [ ] `@tuibridge/ink-shim` package published to npm
- [ ] tui1 can be rebuilt with `--alias:ink=@tuibridge/ink-shim`
- [ ] Rebuilt tui1 runs under `tuibridge` without errors
- [ ] Visual output matches Deno/Ink within ANSI diff tolerance

### Phase 2 (React Reconciler Bridge)
- [ ] `src/runtime_react.js` created with full host config
- [ ] `--react` flag loads React reconciler bridge
- [ ] Bundled React apps (like tui1) run without modification
- [ ] React hooks (useState, useEffect) work through bridge

## Test Plan

```bash
# Phase 1 test
cd ../tui1
npm install @tuibridge/ink-shim
npx esbuild mod.tsx --bundle --alias:ink=@tuibridge/ink-shim --outfile=dist/tuibridge.js
cd -
./target/release/tuibridge ../tui1/dist/tuibridge.js

# Phase 2 test
./target/release/tuibridge --react ../tui1/dist/bundle.js
```

## Dependencies
- Task 060 (compat validation — same FFI surface)
- Task 062 (props propagation — may share BridgeConfig pattern)

## SPEC Reference
§2 Stack (Bridge layer); §4 JS Runtime

## Notes

**Why not just use TuiBridge's reconciler?**

TuiBridge's `runtime.js` reconciler is ~300 lines and handles basic React patterns. However:
- It doesn't implement React's full concurrent features
- It doesn't support React DevTools
- It doesn't handle all edge cases ( Suspense, error boundaries, portals)

For simple apps, TuiBridge's reconciler is sufficient. For complex apps (like tui1 with React+Ink), a proper React reconciler bridge is needed.

**Bundle size concern:**

React + react-reconciler adds ~40KB gzipped. For a terminal app, this is acceptable. TuiBridge's current runtime.js is ~30KB (1,070 lines). The React bridge would be optional, loaded only with `--react`.
