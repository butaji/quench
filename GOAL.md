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
- track work in tasks/*
- commit and push as you progress
