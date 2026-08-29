# Vertical slice, then spec-dependency clusters

The path to a green spec suite starts with a wast harness and validator directives (`assert_malformed`, `assert_invalid`), then one execute slice (`assert_return` of an `i32` export), then clusters in dependency order (control/memory/linking, bulk+refs, SIMD, memory64, GC, exceptions, threads last). The runner scores each wast directive, not only each file.

**Considered Options**: implement every Native opcode before measuring; pick the next failing file with no cluster plan.
