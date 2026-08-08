# GOAL

## Goal

- 100% ECMA-262 test262 on quench-runtime — staged, pinned to the
  submodule commit in `tasks/index.json`, no undocumented skips.
- North star (ADR 0003): two planes — ECMAScript execution (always
  correct) + persistent TypeScript semantic plane, meeting only through
  guards; compact bytecode as the canonical execution format long-term.
- Budget: ~100k Rust LOC ceiling (ADR 0003 "Scope budget"); minimum RSS
  and startup time at the interpreter tier; V8-grade throughput via
  later tiers (baseline compiler as a separate crate), not promised from
  the interpreter alone.
- Boundaries: quench-runtime = pure engine; quench-test262 = runner;
  host interface only between them.

## Meta — learn and accelerate as it runs

The process itself is part of the goal: every iteration must leave the
workflow faster than it found it.

- After each turn, find 3 things to improve in tools / instruments /
  approach that would move faster toward the goal — and implement them
  before moving to the next turn.
- Improvements are durable, not one-off: encode them in `tools/`,
  `tasks/`, `AGENTS.md`, or hooks — never leave a learned trick only in
  conversation history.
- Measure before and after: a tool change that doesn't shorten the
  fix-verify loop (stage run time, time-to-diagnose a failure digest,
  time-to-green) gets reverted, not kept.
- Compound: prefer improvements that make future improvements easier
  (better digests, faster stage runs, sharper diagnostics) over local
  shortcuts.

## Process

- Track work in `tasks/*` (`index.json` = stage SSOT, `refactor-plan.md`
  = work queue).
- Commit and push as you progress.
