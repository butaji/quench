# Preserve Node error codes across the host boundary

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

Make native failures expose the Node-compatible error type, code, message, and relevant fields.

## Scope

- Audit host callbacks in `crates/quench-node/src/main.rs`.
- Replace generic errors where Node behavior requires codes such as `ENOENT`, `EINVAL`, or `ERR_INVALID_ARG_VALUE`.
- Keep path, permission, encoding, and validation behavior consistent with the JavaScript polyfills.
- Add focused negative tests for each corrected contract.

## Done when

- Error-focused compatibility stages pass.
- Existing successful paths remain unchanged.

## Status

Filesystem `chmodSync()` host failures now cross the boundary as Node-shaped
`ENOENT` errors, covered by `tests/node-compat/stage-2060`.

Missing-file `ENOENT` metadata is covered by
`tests/node-compat/stage-376/error-enoent.js`. Additional permission,
validation, and encoding error contracts remain in progress.
Invalid object paths are covered by
`tests/node-compat/stage-377/error-invalid-path.js`.
Write-path validation is covered by
`tests/node-compat/stage-378/error-invalid-write-path.js`.
Unknown read encodings are covered by
`tests/node-compat/stage-379/error-unknown-encoding.js`.
Unknown file descriptors are covered by
`tests/node-compat/stage-387/fs-close-invalid-fd.js`.
Asynchronous descriptor closure is covered by
`tests/node-compat/stage-390/fs-close-callback.js`.
Missing-file `statSync()` metadata is covered by
`tests/node-compat/stage-419/fs-stat-enoent.js`.
Synchronous `fs.readFile()` callback validation is covered by
`tests/node-compat/stage-479/fs-readfile-callback-validation.js`.
Synchronous `fs.mkdtemp()` callback validation is covered by
`tests/node-compat/stage-480/fs-mkdtemp-callback-validation.js`.
Synchronous `fs.mkdtempSync()` prefix validation is covered by
`tests/node-compat/stage-481/fs-mkdtemp-prefix-validation.js`.
Asynchronous `fs.mkdtemp()` prefix validation is covered by
`tests/node-compat/stage-482/fs-mkdtemp-async-prefix.js`.
Synchronous and asynchronous `fs.mkdtemp()` options validation is covered by
`tests/node-compat/stage-483/fs-mkdtemp-options-validation.js`.
The internal event-target weak-handler symbol is exposed for Node's internal
compatibility tests and covered by `tests/node-compat/stage-492`.

`rmdirSync()` now preserves Node error metadata for missing paths, files, and
permission failures (`ENOENT`, `ENOTDIR`, `EACCES`, and `ENOTEMPTY`), covered by
the corresponding upstream rmdir fixtures. BigInt stat conversion now covers
`stat`, `lstat`, `fstat`, promise APIs, and `FileHandle.stat(options)`;
`test-fs-stat-bigint.js` passes, including numeric and nanosecond fields.

Recursive `fs.rmdir()` is rejected with `ERR_INVALID_ARG_VALUE` across sync,
callback, and promise forms, covered by the authoritative
`test-fs-rmdir-recursive-error.js` fixture.
