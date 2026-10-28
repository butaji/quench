# Typed Wasm import callout screen

This three-round screen evaluated a typed ABI for Wasm imports with signature
`(i32, i32, i32) -> ()`. The hypothesis was that passing the three values in
registers would remove caller-side widening, stack stores and the unused
result slot from Quench's generic Cranelift callout. The adapter still used
the ordinary VM import-callout entry for semantics.

The GCC-Wasm fixture passed validation and output equality in all four
reports. The measured host was Linux x86_64, with a four-CPU quota and a 16 GiB
cgroup limit. The corpus revision was
`06785cf861ac44855f168cbbe829278c2802e6de`; Quench source revision was
`5fd297e6ee0d2f0f1f50b2a7e6c6aa796423784d`, with the prototype patch in
`typed-callout.patch`. The reports are diagnostic screens (`source_dirty=true`,
`qualification_ready=false`), not peer qualification evidence.

| Mode | Generic ABI Score / max RSS | Typed ABI Score / max RSS | Typed / generic ratio |
| --- | ---: | ---: | ---: |
| Full JIT | 43.78 / 48.19 MiB | 42.36 / 48.18 MiB | Score 0.968; RSS 1.000 |
| Selective JIT | 57.34 / 48.29 MiB | 57.11 / 48.15 MiB | Score 0.996; RSS 0.997 |

The typed form did not reach the predeclared 5% Score advancement bar and is a
no-go. These separate three-round windows do not establish a statistically
supported regression; they show no basis for a longer confirmation. A likely
mechanical explanation is that the adapter moved argument stores into an
additional Rust call rather than removing the full cost of materializing the
Wasm argument words.

The exact control and candidate reports are:

- `gcc-typed-callout-off-screen-3.json`
- `gcc-typed-callout-on-screen-3.json`
- `gcc-typed-callout-selective-off-screen-3.json`
- `gcc-typed-callout-selective-on-screen-3.json`

The two pairs were run with identical commands except for the typed-callout
setting:

```sh
QUENCH_WASM_JIT_TYPED_CALLOUTS=0 target/production/quench-bench \
  --fixtures gcc-loops-wasm --suite jetstream3 --jit --jit-diagnostics \
  --runs 3 --timeout-ms 600000 --quench target/production/quench-node \
  --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun

QUENCH_WASM_JIT_TYPED_CALLOUTS=1 target/production/quench-bench \
  --fixtures gcc-loops-wasm --suite jetstream3 --jit --jit-diagnostics \
  --runs 3 --timeout-ms 600000 --quench target/production/quench-node \
  --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun
```

The selective pair adds `--jit-wasm-selective` to each command.
