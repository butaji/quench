# Native generation reference

The JIT-capable VM generator paper (https://arxiv.org/abs/2411.11469)
derives an interpreter, baseline JIT and tier switching from bytecode semantics.
Its implementation uses C++ semantics and compiler machinery; Quench's Rust macros
must provide explicit equivalents, not claim the paper's capabilities by naming.

| Adopted idea | Quench implementation direction |
| --- | --- |
| Shared bytecode semantics | Rust declarations generate mechanical interpreter/JIT views over shared helpers |
| Specialization and quickening | Proven facts or guarded representations; correct miss and invalidation paths |
| Register retention and check elimination | Explicit ABI, liveness, effects and cross-fragment transfers |
| Inline caches | Shared receiver/callee/dependency facts with correct polymorphism |
| Hot/cold separation and OSR | Native blocks, helper boundaries and exact frame/PC reconstruction |

[Copy-and-patch compilation](https://arxiv.org/abs/2011.13127) supplies the template
publication model. rustc optimizes templates offline; runtime patching alone does
not perform cross-template optimization. Quench must implement the analyses that
justify composition, placement and elimination. No paper score transfers to JS
or this M4 without measurement.

The [JIT contract](stencil-jit-implementation-spec.md) defines requirements;
the queue's `critical_path` defines implementation order. Profiled and host
lanes are deliberately non-blocking.
