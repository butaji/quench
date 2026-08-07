# Current compatibility push — stream/iter and auditable baselines

## Verified progress

- Focused contracts: **1,945/1,945 passing**.
- Live inventory: 58 canonical modules, 57 statically registered, one
  platform-limited runtime omission (`node:sea`), and 186 observed Node globals.
- Latest completed differential: 4,682 fixtures with 922 exact matches, 2,461
  Quench-only failures, 537 output mismatches, 87 timeouts, and 190 fixtures
  explicitly classified as environment-limited.
- `stream/iter` now covers broadcast cancellation/abort propagation,
  `fromWritable()`, and preservation of typed-array chunks in `array()` and
  `arraySync()`.

## Current verification

- The fresh full differential completed against canonical `main`: 4,682
  fixtures, 922 exact matches, 1,399 both-failed, 2,461 Quench-only failures,
  537 output mismatches, 180 Node-only failures, 87 timeouts, and 190 explicitly
  environment-limited fixtures. No worker failed.
- Deno formatting, `cargo build -p quench-node`, and `git diff --check` pass.

## Next queue

- Refresh the decision report from the completed differential.
- Continue the owned `streams-events-async` queue, using isolated upstream
  fixtures before changing shared stream semantics.
- The current top owned queue is HTTP (56 callback failures); raw TCP framing
  cases are classified individually when their missing host transport is the
  actual cause.
- Preserve explicit platform classifications for native TLS, HTTPS, HTTP/2,
  inspector, QUIC, and other host-integrated APIs.
