# Complete low-copy Buffer binary paths

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
