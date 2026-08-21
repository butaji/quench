# Add Buffer-based fs read and write paths

> Contract: This task is part of broad Node 24 compatibility across Linux
> x86_64, Linux ARM64, macOS, and Windows. Native addons and Node-API are
> excluded. Use the statuses and release gates in
> [compatibility-contract.md](../docs/compatibility-contract.md).

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

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
Write-stream encoding and `bytesWritten` accounting are covered by
`tests/node-compat/stage-426/fs-write-stream-options.js`.
Read-stream descriptor cleanup before `close` is covered by
`tests/node-compat/stage-427/fs-read-stream-close.js`.
Write-stream descriptor cleanup before `close` is covered by
`tests/node-compat/stage-428/fs-write-stream-close.js`.
Write-stream append flags are covered by
`tests/node-compat/stage-443/fs-write-stream-append.js`.
`autoClose: false` descriptor retention is covered by
`tests/node-compat/stage-430/fs-stream-autoclose.js`.
Write-stream `autoClose: false` retention is covered by
`tests/node-compat/stage-431/fs-write-stream-autoclose.js`.
An in-process HTTP compatibility layer now supports basic server/client
request flow, response headers, encoding, and completion events. This is
covered by `tests/node-compat/stage-494`.
