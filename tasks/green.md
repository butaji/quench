# Green Node API coverage

Target: Bun green modules assert, buffer, console, dgram, dns, events, fs, http, os, path, punycode, querystring, readline, stream, string_decoder, timers, tty, url, zlib.

Rules: file <=500 lines, function <=40 lines, cognitive complexity <=10. Preserve upstream submodules. Each module requires implementation evidence and a focused fixture or upstream fixture result.

Audit checklist (Bun Node.js compatibility page):
- dgram: `addMembership()` requires a bound socket; verify bind-before-membership and error behavior.
- dns: cover resolver APIs and document/fixture the missing `resolveTlsa` surface.
- events: verify listener ordering, once/remove semantics, and EventEmitterAsyncResource asyncId behavior.
- fs/http: exercise Stats metadata and server listen option edge cases (`fd`, handle, `ipv6Only`, signal, keepAlive).
- path/stream: cover `matchesGlob()` semantics and Node-stream-only status predicates.
- console/string_decoder/tty: verify stream destinations, decoder end/subclass behavior, and non-TTY fd construction.
- zlib: cover sync/async and stream compression/decompression/error paths.

Verification: run-compat, applicable tests/node fixtures, cargo test --workspace, tools/lint-rust.sh.