# Reduce crypto byte and string conversions

## Contract alignment

This task supports the Node 24 application-runtime contract on Linux x86_64;
observable Node behavior remains the compatibility target.
See `docs/authoritative-test-sources.md` for the Node, LLRT, Deno, WPT, and
Test262 reference roles.

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
PBKDF2 callback argument validation is covered by
`tests/node-compat/stage-392/crypto-pbkdf2-validation.js`.
Supported algorithm discovery is covered by
`tests/node-compat/stage-395/crypto-capabilities.js`.
Byte comparison and unequal-length validation are covered by
`tests/node-compat/stage-396/crypto-timing-safe-equal.js`.
Random integer generation and range validation are covered by
`tests/node-compat/stage-397/crypto-random-int.js`.
Asynchronous random buffer filling is covered by
`tests/node-compat/stage-398/crypto-random-fill.js`.
Base64 digest output for SHA-256 and HMAC is covered by
`tests/node-compat/stage-399/crypto-digest-base64.js`.
String encodings for hash and HMAC updates are covered by
`tests/node-compat/stage-400/crypto-update-encoding.js`.
Hash and HMAC state branching is covered by
`tests/node-compat/stage-420/crypto-copy.js`.
Hash and HMAC finalized-state errors are covered by
`tests/node-compat/stage-421/crypto-finalized.js`.
HMAC digest encoding validation is covered by
`tests/node-compat/stage-484/crypto-hmac-encoding-validation.js`.
Hash digest encoding validation is covered by
`tests/node-compat/stage-485/crypto-hash-encoding-validation.js`.
Finalized hash and HMAC copy validation is covered by
`tests/node-compat/stage-486/crypto-copy-finalized.js`.
`randomBytes()` size and callback validation is covered by
`tests/node-compat/stage-487/crypto-random-bytes-validation.js`.
`randomFillSync()` buffer and range validation is covered by
`tests/node-compat/stage-488/crypto-random-fill-validation.js`.
