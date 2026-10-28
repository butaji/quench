# JetStream 3 `web-ssr` JIT interpreter attribution

The stock JetStream 3 driver ran with one diagnostic round on Linux x86_64.
The report retains full matched samples, engine proofs, output equality, and
opt-in per-function counters:

- [`web-ssr-interpreter-time-1round.json`](web-ssr-interpreter-time-1round.json)

The invocation used the production `quench-node --jit`, Node v26.10.0, and Bun
1.4.2 with JIT enabled. Node's probe reported Maglev and TurboFan compilation;
Bun reported `useJIT=true`. The corpus was the pinned JetStream 3.0 checkout at
`06785cf861ac44855f168cbbe829278c2802e6de`. The report is a dirty-source,
one-round diagnostic screen and is not qualification evidence.

All engines returned valid scores and equal semantic output. Quench scored
9.55 with 128.62 MiB peak RSS; Node scored 132.77 with 215.15 MiB; Bun scored
135.81 with 231.32 MiB. Quench remains behind both JIT peers on score and below
both on RSS.

The new counters time synchronous interpreter exits after the JIT compile or
fallback decision. They exclude compilation and native execution, include
called functions, and omit exceptional or suspended activations. Inclusive
times overlap between callers and callees, and the diagnostics add overhead.
Across this run, 1,980 rows included one compiled JavaScript function and 61
native entries. The largest per-row interpreter opportunities were two
repeated eval functions:

| Function | Entries | Completed | Inclusive interpreter time | Rejection |
| --- | ---: | ---: | ---: | --- |
| `Ru` | 15,525 | 15,504 | 4.91 s | `bytecode-limit` |
| `Eu` | 15,524 | 15,502 | 4.53 s | `unsupported-activation-or-function-kind` |

Their source names are generated eval identifiers (`<Eval:...>`). These
measurements rank follow-up compiler work; the overlapping times are not
comparable to the process wall time.

Command:

```sh
target/production/quench-bench web-ssr --suite jetstream3 --jit --jit-diagnostics \
  --runs 1 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-2026-10-19/web-ssr-interpreter-attribution/web-ssr-interpreter-time-1round.json
```
