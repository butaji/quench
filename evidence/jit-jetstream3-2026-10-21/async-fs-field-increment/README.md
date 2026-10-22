# Async-fs cached numeric field update

The hot synchronous methods `CountVisitor.visitFile` and `visitDir` lower to
six bytecodes for `this.count++` followed by `return undefined`. A guarded JIT
mode entry path now handles the exact pattern when the receiver is fixed, the
own-field read and strict-store caches both match, and the old value is a
Number. It checks all guards before the write. Accessors, proxies, non-writable
properties, non-number values, and cache misses keep the original interpreter
path.

The iteration-profile diagnostic validated async-fs output and counted
59,119/59,120 fast `visitFile` entries and 200,079/200,080 fast `visitDir`
entries. The first entry to each method warmed the caches in the interpreter.
The diagnostic score is instrumented and is not comparable.

The uninstrumented production JIT screen also validated output:

| Engine | Score | Maximum RSS |
| --- | ---: | ---: |
| Quench JIT | 3.09 | 891 MiB |
| Node JIT | 73.53 | 209 MiB |
| Bun JIT | 137.27 | 66 MiB |

The previous one-round Quench screen was 3.44 / 891 MiB, with Node at 73.60 /
210 MiB and Bun at 128.96 / 67 MiB. Peer scores moved between samples, so
these screens do not show a suite-level gain or loss. Quench remains far behind
both peers on score and uses more RSS.

A separate seven-sample microbenchmark alternated one million calls to the
guarded method and an equivalent interpreted method with one extra unused
constant. Median times were 57.86 ms and 193.68 ms. This supports the local
fast path, but does not predict a JetStream Score gain.

Raw reports are `diagnostic-iteration-1round.json` and
`production-screen-1round.json`.
