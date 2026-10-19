# Clean sampled native-entry diagnostic

This one-round selective-tier diagnostic validates output and records sampled
inclusive wall time at native-entry boundaries for the two JIT-mode Wasm
fixtures that had previously beaten peers on no-JIT Score and RSS. It is an
attribution run only: diagnostics add overhead, the sample is one round, and
`qualification_ready` is false. It does not establish a score or RSS result.

The source tree was clean at revision `2a9d4402bc35503a9f218407edb99e42b3ec019b`
and the pinned JetStream checkout was clean at
`06785cf861ac44855f168cbbe829278c2802e6de`. The production Quench binary hash
was `8469a1a28cbfd761a30e169a22f0af43a9db673ca4aa6cd4183a2bc1114de738`.
The report SHA-256 is
`0a4fa752444f8861e67ab5818dc634b7ff50aa63a0cde494495ca54950ccaff5`.

Both fixtures passed validation and output equality. The profile sampled
defined-body indices, which map to absolute Wasm function indices by adding
the function-import count shown in each fixture's `.js.symbols` file:

| Fixture | Body index | Symbol | Entry requests | Samples | Sampled inclusive wall time |
| --- | ---: | --- | ---: | ---: | ---: |
| QuickSort | 1 | `Quicksort` | 3,112 | 12 | 1.203 ms |
| GCC loops | 18 | `example25` | 102,419 | 400 | 0.673 ms |
| GCC loops | 11 | `example10a` | 102,419 | 400 | 0.552 ms |

Recursive entries overlap, so these values cannot be added as exclusive
function costs or read as CPU time. Linked `CallKnown` machine calls bypass the
hook and are charged to their enclosing entry. The purpose is to choose a
function for code inspection, not to explain the whole fixture score.

Reproduce from the repository root after a production build:

```sh
QUENCH_WASM_JIT_STATS=1 target/production/quench-bench \
  --fixtures gcc-loops-wasm,quicksort-wasm --suite jetstream3 \
  --jit --jit-wasm-selective --jit-diagnostics --runs 1 \
  --timeout-ms 600000 --quench target/production/quench-node \
  --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out target/production/native-entry-sampling-clean.json
```

The raw report is `clean-one-round.json`.
