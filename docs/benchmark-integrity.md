# Benchmark integrity

Quench's product is a Node-compatible JavaScript runtime, not a benchmark
adapter. Benchmark suites are useful measurement inputs, but they are never
runtime inputs or semantic authorities.

## Hard boundary

- `quench-node` contains the Node host and compatibility APIs only. It does not
  recognize benchmark filenames, suite markers, source text, or score harnesses,
  and it never delegates execution to Bun, Node, V8, or another engine.
- `quench-runtime` executes JavaScript semantics. It may use a general-purpose
  optimization when reusable IR facts prove its preconditions, and a miss must
  return to the ordinary VM. A fast path must not know that a program came from
  V8-v7/Octane or any other benchmark.
- Expected scores, checksums, benchmark-only constants, whole-program snapshots,
  and workload-specific native implementations are forbidden in production
  runtime code.

## Review checklist

For every performance change, reviewers must be able to answer “yes” to each:

1. Can the optimization trigger on an unrelated Node program with the same
   proven IR shape?
2. Does it preserve observable calls, property access, mutation, exceptions,
   coercions, allocation, and ordering?
3. Does an `Unknown` or failed guard execute complete ordinary semantics?
4. Is there no benchmark filename/source/score dependency anywhere on the
   production path?
5. Are Node-compat tests the acceptance evidence, with benchmark results kept
   as measurements only?

If any answer is “no”, remove the specialization. Do not hide it behind an
environment variable, build profile, generated binary, or test-only switch.

The performance-lane instrumentation described in
[`performance-lanes.md`](performance-lanes.md) is diagnostic data only; it is
not permission to add benchmark-shaped execution paths.
