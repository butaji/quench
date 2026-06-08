# Task 138: Handle Dynamic Import of Node Built-ins (`import("node:readline")`)

**Priority:** P0-Critical
**Phase:** 12 — Real-World Validation
**Depends on:** 132, 073

## Problem

The `../tui1` example dynamically imports a Node.js built-in module:

```tsx
import("node:readline").then(({ createInterface }) => {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  // ...
});
```

In rquickjs:
1. `import()` of `"node:readline"` will fail (no filesystem access to node_modules)
2. Even if it worked, `readline` is a Node.js API not available in rquickjs

In compile path:
1. Dynamic imports are not yet fully supported in codegen

## Strategy

For **dev path** (rquickjs): The `import("node:readline")` call should either:
- Be polyfilled with a no-op that returns `{ createInterface: () => ({}) }`
- Throw a catchable error so the `.catch()` handler can degrade gracefully

For **compile path**: Dynamic imports of node built-ins should be resolved at build time or replaced with stubs.

## Acceptance Criteria

- [ ] `import("node:readline")` does not crash rquickjs (returns rejected Promise or stub)
- [ ] `.then()` / `.catch()` handlers on dynamic imports execute correctly
- [ ] `../tui1` example continues past the dynamic import without uncaught exception
- [ ] Compile path generates valid Rust for dynamic imports (even if stubbed)
