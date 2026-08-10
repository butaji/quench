# Complete low-copy Buffer binary paths

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

Make Node-compatible `Buffer` operations use typed-array/ArrayBuffer data without converting through hexadecimal or JavaScript strings.

## Scope

- Inspect the existing Buffer polyfill and related stages.
- Implement `Buffer.from(ArrayBuffer)`, `subarray`, `slice`, `copy`, and `concat` with correct sharing/copying semantics.
- Preserve Node validation, offsets, lengths, and error behavior.
- Add a focused stage under `tests/node-compat/stage-N/`.

## Done when

- Binary operations pass focused compatibility tests.
- No public API changes are introduced.
- The stage is run, formatted, checked, committed, and pushed according to the repository workflow.

## Status

Implemented and verified by `tests/node-compat/stage-362/buffer-binary-sharing.js`.
Buffer now exposes Node's legacy slice/write method names used by generic
Buffer compatibility tests. This is covered by
`tests/node-compat/stage-497`.
Buffer's internal helper names are hidden from reflection on the public
prototype while preserving generic method behavior and `instanceof Buffer`.
This is covered by `tests/node-compat/stage-498`.
Generic Buffer inspection now labels plain `Uint8Array` receivers correctly,
covered by `tests/node-compat/stage-499`.
Float read/write offsets now use Node-compatible validation messages and error
codes, covered by `tests/node-compat/stage-500`.
Invalid `Buffer.from()` received-value diagnostics and `ERR_INVALID_ARG_TYPE`
are covered by `tests/node-compat/stage-1026`.
