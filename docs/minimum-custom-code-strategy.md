# Minimum Custom Code Strategy

> How to reach 100% JS/TS conformance with the smallest possible custom codebase.

## Principle

Every line we write is a line we maintain. The fastest path to 100% conformance is to **reuse battle-tested components** for everything that is not Quench-specific, and to **keep the execution model as simple as possible** (AST → direct interpreter) until the conformance suites pass.

## What to reuse

| Area | Reuse | Avoid writing |
|------|-------|---------------|
| Parser / TS/JSX stripping | `swc_ecma_parser` (already used) or `oxc_parser` | Custom lexer, parser, or type checker |
| Test262 runner | `test262-harness` (npm) + `test262-stream` | Custom test262 discovery / runner |
| TypeScript cases | `typescript` compiler baseline JS output (already used) | TS type checker at runtime |
| Ordered object properties | `indexmap` | Custom insertion-order map |
| String interning | `lasso` or `string-interner` | Custom atom table |
| BigInt | `num-bigint` | Arbitrary-integer arithmetic |
| Date | `chrono` (already used) | Custom date/time logic |
| RegExp | `regex` crate + JS-syntax adapter | Full regex engine |
| JSON | `serde_json` (already used) | Custom JSON parser/stringifier |
| Module resolution | `oxc_resolver` | Custom Node resolution |
| Errors | `thiserror` (already used) | Custom error hierarchies |
| Arena allocation | `bumpalo` | Custom allocators |

## What to simplify in our own code

1. **Collapse the value model.**
   Instead of separate `Function`, `NativeFunction`, `NativeConstructor`, `Array`, etc. variants, use a single `Value::Object` with `[[Call]]` / `[[Construct]]` slots. This is the standard ECMAScript model and removes a lot of custom branching.

2. **Stay AST → interpreter.**
   Do not build a separate HIR or bytecode layer until 100% conformance is reached. The recursive AST interpreter is the minimum custom code path; the trampoline rewrite (Task 85) only changes *how* the AST is walked, not the representation.

3. **Borrow built-in semantics from crates.**
   `Date`, `RegExp`, `JSON.stringify`, `parseInt`, etc. should delegate to Rust ecosystem crates where possible. The custom layer should only enforce ECMAScript edge cases and error messages.

4. **Use the conformance suites as the backlog.**
   Do not write exhaustive spec tests from scratch. Run test262 and the TypeScript baseline suite, bucket failures, and fix the smallest runtime change that makes each bucket pass.

## What this rules out (for now)

- A separate bytecode VM or register machine.
- A custom TypeScript type checker.
- A custom JS parser or lexer.
- Writing built-in algorithms from scratch when a crate covers the semantics.

## Exit criteria

This strategy succeeds when:

- The runtime has zero custom parser/lexer code.
- test262 and TypeScript suite failures are the primary source of new work.
- Built-ins delegate to crates for the heavy lifting.
- The value model has one object representation with callable/constructible slots.

