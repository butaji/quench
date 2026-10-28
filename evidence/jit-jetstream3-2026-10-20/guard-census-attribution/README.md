# JetStream guard census attribution

This is an opt-in, one-round attribution run on commit `0c6fcb431`, using the
pinned JetStream 3.0 corpus and the iteration-profile `quench-node` build.
Both selected fixtures validated and matched outputs. The run is not
qualification evidence: diagnostics add overhead, it has one round, and the
iteration profile is not the production profile.

The census records bytecode sites only. It does not prove that a predicate is
pure or redundant, and the inclusive interpreter time can count nested calls
more than once.

## Findings

- In `hash-map`, `computeHashCode` entered native code 64.8M times (9
  conditional branches, 1 `typeof`, 6 strict comparisons, 1 relational
  comparison). `equals` entered native code 53,999,969 times out of 54M
  entries (6 branches, 4 `typeof`, 4 strict comparisons).
- `_findKeyEntry` ran 54M times with no native entries. It has a loop and was
  rejected for unsupported opcode/control flow. This is a stronger candidate
  for investigation than rewriting the already-native type-dispatch helpers.
- In `web-ssr`, high inclusive interpreter-time functions were rejected for
  bytecode limits or unsupported activation/function kind. Their census
  contains many branch sites, but these counts do not show redundant guards.

These results do not identify a proven, small pure-guard region for ISOP. The
next experiment should target the hot unsupported loop or native-call
coverage, while keeping exact interpreter fallback and deoptimization state.

## Reproduction

```sh
target/iteration/quench-bench \
  --fixtures web-ssr,hash-map --suite jetstream3 --jit --jit-diagnostics \
  --runs 1 --timeout-ms 600000 \
  --quench target/iteration/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun/node_modules/@oven/bun-linux-x64/bin/bun \
  --qjs /bin/true \
  --out target/js-guard-census-1round.json
```

The raw benchmark report is
[`jetstream3-jit-guard-census-1round.json`](jetstream3-jit-guard-census-1round.json);
the compact function summary is
[`guard-census-summary.json`](guard-census-summary.json).
