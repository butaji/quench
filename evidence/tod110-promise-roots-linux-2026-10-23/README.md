# Promise root append screen

This is a focused JetStream 3 `doxbee-promise` experiment on Linux x86_64. The
change replaces `PromiseRecord::roots()`'s nested iterator chain with direct
ordered pushes into the collector's existing work vector. It preserves the
edge sequence: `result`, followed by each reaction's fulfilled, rejected and
next values.

The baseline binary was built from clean source revision `5a26eb1e0441` with
`cargo build --locked --profile production -p quench-node -p quench-bench`.
The candidate binary was built from that same source plus exactly the change
landed as `37b84eda0`; its reports retain the runner's base `source_revision`
field because the build preceded the commit. The final committed source is
byte-for-byte the source used for the candidate build.

Host: Linux `6.18.55`, x86_64; rustc `1.99.0 (b940084d7 2026-09-28)`. Node was
the installed v24.19.0 runtime; Bun was 1.4.2. Every report records
`output_equal: true` and valid Quench, Node and Bun outputs.

## Five-round screen

| Mode | Binary | Quench score / RSS | Bun score / RSS | Node score / RSS |
|---|---|---:|---:|---:|
| no-JIT | baseline | 25.30 / 59.7 MiB | 62.84 / 43.5 MiB | 54.19 / 178.8 MiB |
| no-JIT | candidate | 26.10 / 59.8 MiB | 62.06 / 44.0 MiB | 57.04 / 178.6 MiB |
| JIT | baseline | 21.21 / 58.4 MiB | 176.73 / 98.5 MiB | 78.64 / 267.6 MiB |
| JIT | candidate | 28.88 / 58.4 MiB | 202.08 / 96.9 MiB | 98.79 / 269.3 MiB |

The batches are sequential, not paired. Peer scores shifted substantially,
so the apparent candidate gain is confounded by run-to-run drift. Ratios to
the peers moved by +4.4% / -2.0% in no-JIT and +19.1% / +8.4% in JIT (Bun /
Node respectively); this is not a clean candidate-vs-baseline result.

## Eleven-round candidate screen

| Mode | Quench score / RSS | Bun score / RSS | Node score / RSS |
|---|---:|---:|---:|
| no-JIT | 19.78 / 59.7 MiB | 50.92 / 43.7 MiB | 50.45 / 178.7 MiB |
| JIT | 22.04 / 58.4 MiB | 153.59 / 97.9 MiB | 66.74 / 267.6 MiB |

Both reports have 11/11 valid rounds and output parity. The eleven-round
candidate run does not beat either peer on score. Its JIT RSS is lower than
both peers; no-JIT RSS is higher than Bun's. This is a single-fixture screen,
not a JetStream suite qualification or evidence for the ≥1.20× target.

## Decision

Keep this small GC hot-path change as an experiment because it is semantics
preserving and showed a promising JIT screen without increasing Quench RSS.
The score effect is not isolated, and it does not establish a broad no-JIT or
JIT win. Follow up with a paired multi-fixture comparison before making any
performance claim. Full runtime validation passed: 827 `quench-runtime` lib
tests under the iteration profile.
