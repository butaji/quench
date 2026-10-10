# v2-cloud-a Stage A integration checks (2026-10-10)

The fetched `v2-cloud-a` branch adds Node host bindings and fixtures across
process, filesystem, URL, networking, crypto, module loading, and related APIs.
Its production merge builds with Rust 1.99.0. The resulting binary SHA-256 is
`3f287358b54b56ef4ce3b439172ad114a4f09ed052fb6e3a1f9c069ff3fd3b53`.

Node v24.19.0 and Quench produced identical output for the focused
`import.meta.url` and `IteratorClose` probes. The iterator probe confirms that a
primitive from an iterator's `return()` produces `TypeError` after the close
call, while an object return completes normally.

The pinned `framework-core` profile passed 43/43 fixtures with 120-second
per-fixture deadlines, no skips, crashes, or failures. Under the 30-second
deadline, 42 passed and `test-string-decoder.js` timed out; that fixture passed
in isolation with a 120-second deadline. This records Linux host timing under
the current 4-CPU quota and does not replace the official M4 Stage A report.

Raw probe outputs and both profile runs are in this directory. The focused
source probes are `import-meta.mjs` and `iterator-close.cjs`.
