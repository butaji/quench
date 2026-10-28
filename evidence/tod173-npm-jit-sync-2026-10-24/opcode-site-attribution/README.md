# General JIT dynamic opcode sites

The diagnostic counter records dynamic general-dispatch counts by function,
bytecode PC, and opcode under `QUENCH_JS_JIT_STATS=1`. Normal executions do
not allocate or update these maps. These reports use production-mode builds
without PGO, the npm fixtures as workloads only, and `QUENCH_JS_JIT_GENERAL_DISPATCH=1`.
They are attribution data, not performance comparisons or PGO training.

Diagnostic binary SHA-256:
`e8763fd9e90f53b25b3c8342811f79ecb44d5783c13e9ea0e9ae54a9327f4028`

| Workload | Function | Dominant dynamic sites |
|---|---|---|
| React SSR | `pushStartGenericElement` | `GetIterator` PC 14: 504,380 dispatches over 2,500 rounds. This is the `for...in` over props. |
| Zod parse | `operations` | `GetField` PCs 38, 43, 52, 77, 78, and 79: 1,050 dispatches each over 1,000 rounds; `Binary` PC 80: 1,050. |
| Pino logger | `getValueIfExists` | `Binary` PC 34: 15,750; `GetIterator` PC 2: 10,499. |
| Pino logger | `_asJson` | `GetIterator` PC 60: 5,243 over 5,000 rounds. |

The React site is the strongest next compiler target: a single site dominates
general-dispatch activity, while previous generic `GetIterator` branches were
rejected because they added overhead at all sites. The next implementation
should use generic per-function site feedback and guard this exact operation's
iterator semantics; it must not encode the npm function name or PC as a
workload-specific runtime rule. Preserve normal VM fallback for every
unsupported iterator, mutation, callback, and abrupt completion.

The `.err` files include complete per-function per-PC counts. The `.out` files
record workload result checksums and timings from attribution runs.
