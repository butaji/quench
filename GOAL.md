# Goal: stages 36–39

Bring test262 stages 36–39 to 100% passing through the canonical runner:
AsyncFunction, AsyncGeneratorFunction, AsyncGeneratorPrototype, and
AsyncIteratorPrototype. Implement callable/constructable behavior, own
properties, prototype identity, async suspension/resumption, iterator results,
and completion propagation canonically. No stack-overflow workarounds,
harness changes, or test262 edits. Re-run stages 36–39 and earlier regressions
after each fix; finish with zero failures, clean quality checks, and committed
verified changes.
