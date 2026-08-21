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

Current evidence is incomplete: the focused suite is 49/57 with eight
failures, and the upstream parallel run panics in datetime formatting. Do not
mark green complete until relevant Node API runs finish without panic.