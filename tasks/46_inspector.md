# inspector

Bun documents `Session` support for Profiler/precise coverage, `Runtime.enable`,
and NodeTracing, plus `node:inspector/promises`, `open()`, `url()`, `close()`,
and `waitForDebugger()`. Quench currently provides a deterministic Session
lifecycle and callback-based `post`; native transport and the unsupported
`Runtime.evaluate`, HeapProfiler, and Network domains remain gaps. Validate
each claimed method with focused and applicable upstream Node API tests.
