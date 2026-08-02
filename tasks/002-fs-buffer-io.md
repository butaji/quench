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

## Status

Behavioral Buffer round-trip coverage is present in
`tests/node-compat/stage-363/fs-buffer-roundtrip.js`. Native byte transport
is now used by `readFileSync`/`writeFileSync`; positioned and file-handle
paths and append mode now use byte transport as well. Legacy encoding branches
retain only JavaScript-side conversions where required by the public API.

The task is complete for the scoped fs APIs.
FileHandle promise reads from the current descriptor position are covered by
`tests/node-compat/stage-413/fs-filehandle-read-position.js`.
FileHandle read encoding behavior is covered by
`tests/node-compat/stage-414/fs-filehandle-read-encoding.js`.
Numeric descriptor resolution for `readFileSync()` is covered by
`tests/node-compat/stage-415/fs-read-file-fd.js`.
Synchronous callback API data validation for `appendFile()` is covered by
`tests/node-compat/stage-416/fs-append-validation.js`.
Binary `appendFileSync()` transport is covered by
`tests/node-compat/stage-417/fs-append-binary.js`.
Encoded string append transport is covered by
`tests/node-compat/stage-418/fs-append-encoding.js`.
Basic `createWriteStream().end()` lifecycle is covered by
`tests/node-compat/stage-422/fs-write-stream-end.js`.
Basic `createReadStream()` byte delivery is covered by
`tests/node-compat/stage-423/fs-read-stream.js`.
Read-stream encoding and `bytesRead` accounting are covered by
`tests/node-compat/stage-424/fs-read-stream-options.js`.
Read-stream start/end range validation is covered by
`tests/node-compat/stage-425/fs-read-stream-range.js`.
