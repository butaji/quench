# Minimum Custom Code Strategy

> Exact decisions for reaching 100% JS/TS conformance with the smallest possible custom codebase.

## Principle

Every line we write is a line we maintain. The fastest, lowest-risk path to 100% conformance is to **reuse battle-tested components** for everything that is not Quench-specific, and to **keep the execution model as simple as possible** until the conformance suites pass.

These decisions are based on:

- Online research into embeddable JS engines (Boa, QuickJS, Luau, Oxc, SWC).
- The current Quench codebase already using SWC and `serde_json`.
- `context.md` findings that the value model split into `Function`/`NativeFunction`/`NativeConstructor` is the single biggest source of custom code.

## Exact decisions

| Area | Decision | Rationale |
|------|----------|-----------|
| Parser / TS/JSX stripping | **Keep `swc_ecma_parser`.** | Already integrated, passes the required grammar, and the correctness payoff of switching to `oxc_parser` does not justify the migration effort before 100% conformance. |
| Test262 runner | **Adopt `test262-harness` (npm).** | It already implements discovery, frontmatter parsing, helper injection, timeout handling, and reporters. Replacing it with custom Rust code duplicates effort and drifts from the spec format. |
| TypeScript cases | **Keep `typescript` compiler baseline JS output.** | Runtime type checking is out of scope; the TypeScript compiler gives us executable JS for free. |
| Ordered object properties | **Use `indexmap`.** | Standard crate for insertion-ordered maps; exactly what ECMAScript objects require. |
| String interning | **Use `lasso`.** | Mature, `no_std`-friendly interner; turns property/identifier comparison into a `u32` equality check. |
| BigInt | **Use `num-bigint`.** | Proven arbitrary-precision integer crate; we only add sign handling and prototype methods. |
| Date | **Keep `chrono`.** | Already in use; covers epoch arithmetic, formatting, and time zones. |
| RegExp | **Use `regex` crate + a JS-syntax adapter.** | Writing a regex engine is thousands of lines; the adapter layer only handles JS flag/edge-case differences. |
| JSON | **Keep `serde_json`.** | Already in use; handles parsing and stringification. We only implement the ECMAScript-specific `replacer`/`reviver` and edge cases. |
| Module resolution | **Use `oxc_resolver`.** | Dedicated crate for Node-style resolution; smaller and more focused than writing our own. |
| Errors | **Keep `thiserror`.** | Already in use; minimal boilerplate for structured errors. |
| Arena allocation | **Use `bumpalo`.** | Standard arena allocator for AST/HIR nodes. |

## What to simplify in our own code

1. **Collapse the value model.**
   Replace the separate `Function`, `NativeFunction`, `NativeConstructor`, and special-cased `Array` variants with a single `Value::Object` whose internal object record carries `[[Call]]` and `[[Construct]]` slots when applicable. This is the ECMAScript object model and removes the largest source of custom branching in the runtime.

2. **Stay AST → interpreter only.**
   Do not build a separate HIR or bytecode layer until 100% conformance is reached. The trampoline AST interpreter is the minimum custom code path. Task 85 changes *how* the AST is walked (explicit `Vec<CallFrame>`), not the representation.

3. **Delegate built-ins to crates.**
   `Date`, `RegExp`, `JSON.stringify`, `parseInt`, etc. delegate to the crates above. Custom code only enforces ECMAScript edge cases and error messages.

4. **Use the conformance suites as the backlog.**
   Do not write exhaustive spec tests from scratch. Run test262 and the TypeScript baseline suite, bucket failures, and fix the smallest runtime change that makes each bucket pass.

## What this rules out until 100% conformance

- A separate bytecode VM or register machine.
- A custom TypeScript type checker.
- A custom JS parser or lexer.
- Hand-rolled built-in algorithms covered by the crates above.

## Exit criteria

- [ ] The runtime has zero custom parser/lexer code.
- [ ] `test262-harness` runs the full test262 suite.
- [ ] Built-ins delegate to crates for core semantics.
- [ ] The value model is a single object representation with callable/constructible slots.
- [ ] test262 and TypeScript suite failures are the primary source of new work.
