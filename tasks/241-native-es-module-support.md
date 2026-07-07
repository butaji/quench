> **Background process is implementing this.** Update as `import`/module-loader pieces land.

# Task 241: Implement native ES module support for Quench apps

## Status: IN PROGRESS

## Gap

`import` and `export` statements are currently stripped rather than executed. There is no native module loader, module map, or cyclic dependency handling.

## Current progress (in working tree)

- `Context::parse` now uses `parse_auto` to detect module syntax.
- `swc_parse.rs` has a `parse_module` helper.
- `lower.rs` lowers:
  - `export default expr` → `exports.default = expr`
  - `export default function` → function declaration + `exports.default = name`
  - `export { x, y }` → `exports.x = x`, `exports.y = y`
  - `export const/let/var x = 1` → declaration + `exports.x = x`
- `import` statements still return `None` (not executed yet).
- Re-exports (`export * from`, `export { x } from`) are not implemented yet.

## Exact remaining work

1. Create `crates/quench-runtime/src/modules.rs` with:
   - `struct ModuleRecord { source: String, resolved_path: PathBuf, exports: HashMap<String, Value>, evaluated: bool }`
   - `struct ModuleLoader { cache: HashMap<PathBuf, Rc<RefCell<ModuleRecord>>> }`
   - `fn resolve(specifier: &str, referrer: &Path) -> Result<PathBuf, JsError>` for relative (`./foo.js`) and package specifiers.
   - `fn load(path: &Path, ctx: &mut Context) -> Result<Rc<RefCell<ModuleRecord>>, JsError>` that parses, lowers, and evaluates the module body.
2. In `lower.rs`, turn `import { foo } from './bar.js'` into a runtime call or a binding record that the module loader resolves after the dependency is loaded.
3. In the interpreter, execute module bodies inside a module-scope environment. Bind imported names to the exported values of the resolved module record.
4. Support cyclic imports by returning the module record from cache before evaluation starts (ECMA-262 TDZ semantics for uninitialized bindings).
5. Add regression tests in `crates/quench-runtime/tests/modules.rs` for:
   - named import/export
   - default import/export
   - star import (`* as ns`)
   - relative resolution
   - cyclic import

## Acceptance criteria

- [ ] `import { foo } from './bar.js'` loads and binds the export.
- [ ] `export default expr` and `export const x = 1` work.
- [ ] Cyclic imports do not crash and follow ECMA-262 semantics.
- [ ] Regression tests for module loading and binding.

## Files

- `crates/quench-runtime/src/modules.rs` (new)
- `crates/quench-runtime/src/lower.rs`
- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/interpreter/*.rs`
- `crates/quench-runtime/src/lib.rs`

## Tests unblocked

- test262 `language/module-code/`
- TypeScript module conformance cases

## Targets

- **Suite:** `both`
- **Batch:** 4
- **Target subset:** `tests/test262/test/language/module-code/` + TypeScript module conformance cases
- **Blocked by:** 85
- **Exit criteria:** Module loading subsets pass at 100% with zero spec skips.
