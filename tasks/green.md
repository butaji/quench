# Green Node API coverage

Target: Bun green modules assert, buffer, console, dgram, dns, events, fs, http, os, path, punycode, querystring, readline, stream, string_decoder, timers, tty, url, zlib.

Rules: file <=500 lines, function <=40 lines, cognitive complexity <=10. Preserve upstream submodules. Each module requires implementation evidence and a focused fixture or upstream fixture result.

Verification: run-compat, applicable tests/node fixtures, cargo test --workspace, tools/lint-rust.sh.