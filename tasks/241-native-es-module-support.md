# Task 241: Implement native ES module support for Quench apps

## Status: PENDING

## Gap

`import` and `export` statements are currently stripped rather than executed. There is no native module loader, module map, or cyclic dependency handling.

## Fix

- Parse and lower `import`/`export` declarations.
- Implement a module loader that resolves relative and package specifiers.
- Build a module map and execute modules in dependency order.
- Support named exports, default exports, and star imports.

## Acceptance criteria

- [ ] `import { foo } from './bar.js'` loads and binds the export.
- [ ] `export default expr` and `export const x = 1` work.
- [ ] Cyclic imports do not crash and follow ECMA-262 semantics.
- [ ] Regression tests for module loading and binding.

## Files

- `crates/quench-runtime/src/modules.rs` (new)
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/interpreter/*.rs`
- `crates/quench-runtime/src/lib.rs`

## Tests unblocked

- test262 `language/module-code/`
- TypeScript module conformance cases
