# Current compatibility push — stream/iter and auditable baselines

## Verified progress

- Focused contracts: **1,945/1,945 passing**.
- Live inventory: 58 canonical modules, 57 statically registered, one
  platform-limited runtime omission (`node:sea`), and 186 observed Node globals.
- Latest completed differential before the current run: 4,682 fixtures with
  809 exact matches; 183 fixtures explicitly classified as environment-limited.
- `stream/iter` now covers broadcast cancellation/abort propagation,
  `fromWritable()`, and preservation of typed-array chunks in `array()` and
  `arraySync()`.

## Current verification

- The fresh full differential completed against the current source: 4,682
  fixtures, 808 exact matches, 1,562 Quench-only failures, 449 output
  mismatches, 92 timeouts, and 184 explicitly environment-limited fixtures.
  No worker failed. The one-match fluctuation versus the previous run is
  retained as evidence of a small nondeterministic/environment-sensitive
  component, not treated as a compatibility regression without repetition.
- Deno formatting, `cargo build -p quench-node`, and `git diff --check` pass.

## Next queue

- Refresh the decision report from the completed differential.
- Continue the owned `streams-events-async` queue, using isolated upstream
  fixtures before changing shared stream semantics.
- Preserve explicit platform classifications for native TLS, HTTPS, HTTP/2,
  inspector, QUIC, and other host-integrated APIs.
