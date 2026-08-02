# Reduce crypto byte and string conversions

## Goal

Keep Node crypto operations on byte-oriented values across the Rust/JavaScript boundary.

## Scope

- Inspect `crypto` polyfills and existing native hashing/randomness bindings.
- Add byte-oriented paths for hashes, HMAC, random bytes, UUIDs, and supported signing/verification APIs.
- Preserve encodings, stream methods, validation, and error codes.
- Avoid changing algorithms or security policy as part of this task.

## Done when

- Focused crypto compatibility stages pass.
- Byte inputs and outputs avoid unnecessary intermediate strings.

## Status

SHA-256 byte updates now use the native `Vec<u8>` binding and are covered by
`tests/node-compat/stage-374/crypto-hash-bytes.js`. HMAC and signing paths
remain in progress. Native random bytes and fill are covered by
`tests/node-compat/stage-375/crypto-random-bytes.js`.
HMAC-SHA256 byte input is covered by
`tests/node-compat/stage-380/crypto-hmac-bytes.js`.
Default binary digest output is covered by
`tests/node-compat/stage-381/crypto-digest-buffer.js`.
SHA-256 PBKDF2 sync and callback APIs are covered by
`tests/node-compat/stage-391/crypto-pbkdf2.js`.
