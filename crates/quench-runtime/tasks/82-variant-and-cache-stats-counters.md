# 82 — Stencil-variant and image-cache counters in DEEGEN_JIT_STATS

Status: planned

`DEEGEN_JIT_STATS=1` already reports compiled images, native entries, loop entries,
and per-op helper-stencil entries. Once constant-operand stencil variants (task 79) and
the persisted linked-image cache (task 80) exist, their effect needs to be measurable
through the same existing stats surface rather than through new one-off tooling, so
`scripts/perf-cycle.sh rank`/`compare` can track them like every other counter.

Add two counters:

- Variant-selection counts: how many call sites selected a literal-operand variant
  versus a local/local variant, per opcode family (depends on task 79).
- Image-cache hit/miss counts: how many script executions loaded a cached linked image
  versus ran the full AOT pipeline (depends on task 80).

Acceptance: both counters appear in `DEEGEN_JIT_STATS=1` output and in the JSONL
records produced by `scripts/perf-cycle.sh`; `reports/` gains no new ad hoc format,
reusing the existing JSONL/Markdown coverage report structure described in the README.

Related: 79 (constant-operand stencil variants), 80 (persisted linked-image cache),
05 (performance harness), 44 (per-task regression gate).
