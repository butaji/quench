# Preserve Node error codes across the host boundary

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

Missing-file `ENOENT` metadata is covered by
`tests/node-compat/stage-376/error-enoent.js`. Additional permission,
validation, and encoding error contracts remain in progress.
