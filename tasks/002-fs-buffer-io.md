# Add Buffer-based fs read and write paths

## Goal

Allow Node `fs` APIs to move binary data directly between Rust and `Buffer`/`ArrayBuffer` values.

## Scope

- Replace hex-string transport for binary file operations where compatible with the existing API.
- Cover `readFileSync`, `writeFileSync`, file-handle reads/writes, and encoding options.
- Preserve callback, promise, offset, length, position, and error contracts.
- Add focused compatibility stages.

## Done when

- Binary fs tests pass without changing the `fs` API surface.
- Existing text and encoding behavior remains passing.
