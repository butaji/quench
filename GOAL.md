- implement quench-runtime for pure js-runtime
Runtime<
    Heap,
    Collector,
    Allocator,
    Frames,
    Executor,
    Exceptions,
    Environments
  >
- quench-test262 for test262 runner details
with clear boundaries
- OXC --> AST --> Quench IR (design a great one for low mem/rss, but
performance of V8 grade) --> Interpreter
- north star (ADR 0003): two planes — ECMAScript execution plane (always
correct, test262) + TypeScript semantic plane (persistent TypeGraph,
reflection, opt-in validation), meeting only through guards; compact
bytecode as the canonical execution format long-term
- scope ceiling: ~100k Rust LOC for engine + frontend + interpreter +
bytecode + GC + modules + async + builtins; JIT, baseline compiler,
debugger, profiler, source maps, optimizer, and TS language services are
postponed (extension points only, baseline compiler later as a separate
crate) — ADR 0003 "Scope budget"
- track work in tasks/*
- after each turn find 3 things to improve tools/instruments/approach to move faster to the goal, and implement it before moving to the next turn
- commit and push as you progress
