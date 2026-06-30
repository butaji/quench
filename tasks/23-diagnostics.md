# Task 23: Improve diagnostics and error messages

## Goal

Make every Quench error — parser, lowering, runtime, or bridge — as clear and helpful as possible. A user should always know what went wrong, where, and what to do next.

## Files

- `crates/quench-runtime/src/swc_parse.rs`
- `crates/quench-runtime/src/lower/mod.rs`
- `crates/quench-runtime/src/value/error.rs`
- `crates/quench-runtime/src/interpreter/mod.rs`
- `crates/quench-runtime/src/context/mod.rs`
- `src/main.rs`

## Current issues to fix

1. **Parser errors** are currently passed through from swc but may lack file names or readable snippets.
2. **Lowering errors** (e.g., "Optional chaining not supported", "Class expressions not supported") do not include the source location.
3. **Runtime errors** (`TypeError`, `ReferenceError`, etc.) do not include a call stack or the line number where the error originated.
4. **Unsupported features** are sometimes silently dropped (e.g., old `ForOf` returned `None`) or produce generic messages.
5. **Bridge host-function errors** (missing global, wrong argument count) are not consistently explained.

## Steps

1. **Attach source spans to HIR nodes.** Every lowered node should carry a `Span` (file, start line/col, end line/col) from the original source.
2. **Rich parser errors.**
   - Wrap swc errors into a `ParseError` that prints the file path, line/column, a snippet underline, and the swc message.
   - Example:
     ```
     Parse error in examples/use-bridge.tsx:12:5
       12 |   let x =
          |           ^ expected expression, found end of file
     ```
3. **Rich lowering errors.**
   - Change `LowerError` from a plain string to a struct with `message`, `span`, and an optional `help` field.
   - For every unsupported construct, include the location and a suggestion (e.g., "Optional chaining is not yet supported at use-bridge.tsx:8:15. Rewrite as a manual null-check or track Task 14.").
4. **Runtime error stack traces.**
   - Add a call-frame stack to the interpreter that records function name, file, and line for each active call.
   - When a runtime error is thrown, format it with the message and the stack trace.
   - Example:
     ```
     ReferenceError: foo is not defined
       at myFunction (examples/app.tsx:14:9)
       at <anonymous> (examples/app.tsx:20:3)
     ```
5. **Bridge/FFI errors.**
   - If a host function is called with the wrong number or type of arguments, return a clear `TypeError` message.
   - If a JS global called from Rust does not exist, report the name and the call site.
6. **Replace silent drops.**
   - Audit the lowerer and interpreter for any place that silently skips a node (old `return None` patterns).
   - Either implement the node or emit a lowering error with location and a tracking note.

## Boundaries

- Only modify error/diagnostic code in `crates/quench-runtime/src/` and `src/main.rs` glue.
- Do not change runtime semantics while adding diagnostics.
- Do not modify `examples/` or `tests/typescript/` fixtures.

## Pareto & reuse note

- Use `thiserror` (already a dependency) for error enum formatting.
- Evaluate `miette` or `ariadne` for richer source snippets if the hand-rolled formatter becomes too complex.
- Reuse swc's `Span` type for locations instead of inventing a new one.

## Acceptance criteria

- A parse error shows file, line, column, and a readable snippet.
- A lowering error for an unsupported construct shows the exact source location and a helpful message.
- A runtime error (`ReferenceError`, `TypeError`, etc.) includes a stack trace with function names and line numbers.
- No unsupported construct is silently dropped during lowering.

## Verification

```bash
cargo test -p quench-runtime
cargo run -- examples/use-bridge.tsx --prop theme=dark  # force a known gap and check the error message
```
