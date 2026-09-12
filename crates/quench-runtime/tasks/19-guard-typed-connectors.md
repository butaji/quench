# 19 — Guard-typed connectors for arithmetic chains

Status: planned

Extend `StencilState` beyond control-shape connectors (`Connector`, `LoopTop`, `LoopExit`, `ReturnState`) with value-guard connectors such as `Int32Guarded` and `F64Guarded`, built on top of [[10-specialized-arithmetic]]. A guarded arithmetic stencil is typed `Stencil<Int32Guarded, Int32Guarded>`; the untyped fallback stays `Stencil<Connector, Connector>`. Because `compose` only typechecks when the left `Out` matches the right `In`, a run of adjacent guarded operations can only be composed into one unbroken unguarded sequence when every step in the chain is still statically guard-compatible — the type system determines legal guard-elision points instead of a hand-written heuristic deciding how many ops to fuse before re-checking.

This must not become benchmark-specific fusion (constraint already stated in [[04-bytecode-coverage-map]] and [[10-specialized-arithmetic]]): guard connectors are a general property of any op sequence with matching numeric type, not a pattern keyed to one benchmark's operator sequence.

Acceptance: a chain of N int32 operations compiles to one unguarded run with guards only at entry and at any point a non-int32-provable value enters; removing a guard produces a compile error via the connector type mismatch, not a runtime bug; overflow/coercion tests from [[10-specialized-arithmetic]] still pass.
