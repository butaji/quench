# 417 — Register-region rejection census

Status: complete

Turn the physical planner's discarded `Result` values into structured evidence. Count every
final register-region planning attempt and classify rejection by the existing semantic enum:
non-single-trace CFG, insufficient numeric work, unsupported boolean use, missing numeric
value, register pressure, or live values at a backedge. Report the counters only through the
existing diagnostics channel; they must not alter selection or hot execution.

Use the census across all V8v7 components to choose the next general planner relaxation by
reachable execution value, then implement and A/B that single restriction. This is the
routine discipline requested by the optimization roadmap: observe normalized facts, rank one
bottleneck, transform the quoted plan, validate semantics, measure physical code, and only
then run the score gate.

Acceptance: all planner outcomes are accounted for, release tests pass, a complete V8v7
diagnostic run is persisted, and the next task cites exact rejection counts rather than an
intuition about coverage.

## Result

Implemented named diagnostic counters and persisted the eight-suite 20 ms census in
`reports/task417-register-region-rejection-census/census.json`. All 160 release tests pass.
The final planner outcomes were:

- 138 attempts, 11 accepted;
- 93 `NotSingleTrace` rejections;
- 31 `TooLittleNumericWork` rejections;
- 2 `RegisterPressure` rejections;
- 1 `UnsupportedBooleanUse` rejection;
- zero missing-value and live-backedge rejections.

The dominant bucket exposed a gate bug, not a missing code generator: `quote_block` produces
a straight `Seq` with no trace header, while the physical planner required
`trace_header() == Some(start)`. Task 418 fixes that precise restriction and keeps all later
CFG and target checks intact.
