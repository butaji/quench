# Green Node API coverage

Target: Bun green modules assert, buffer, console, dgram, dns, events, fs, http,
http2, net, os, path, punycode, querystring, readline, sqlite, stream,
string_decoder, timers, tty, url, zlib, trace_events, and quic. `tasks/advanced.md`
owns the advanced green modules; this file remains the canonical cross-reference.

Green means Bun documents the module as broadly implemented, not that Quench
has passed Node's complete suite. Each module requires implementation evidence,
a focused fixture, and applicable upstream Node API results.

Bun-specific caveats MUST be recorded: dgram requires bind before
addMembership; dns lacks resolveTlsa and has Resolver differences; fs lacks
Temporal.Instant Stats getters; http ignores selected listen options; path
matchesGlob follows Bun.Glob semantics; stream predicates only understand Node
streams; string_decoder rejects end(string) and subclassing; tty permits
non-TTY construction; zlib/http2/quic have documented upstream failure or
experimental-surface caveats.

Current measured evidence: the focused suite passes 58/58 and the upstream
parallel manifest passes 178/178. These results cover the repository's current
fixtures, not every Bun-documented Node v26 API. New or expanded green claims
still require related Node API tests and recorded results.