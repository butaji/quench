# 58 — Borrowed native-kernel argument ABI

Status: complete

Native kernels previously required an owned `Vec<Value>`. Every call from register bytecode therefore cloned all arguments into a new heap allocation even when the kernel only read one or two arguments.

Implementation: the native function ABI now accepts `&[Value]`. Already-contiguous callers pass their slice directly. Register-backed calls use a fixed `INLINE_NATIVE_ARGUMENT_CAPACITY` stack array for up to eight arguments and retain the materialized-vector fallback for larger calls. Array/string kernels that retain or mutate argument values clone only at the actual ownership boundary. `Function.prototype.call` forwards its tail slice without rebuilding a vector.

Correctness: all 32 tests pass after migrating every native kernel to the borrowed ABI.

Measurement: `reports/borrowed-native-args-ab/comparison.txt` records seven-suite alternating medians of 501.500→508.215 (+1.34% aggregate). Crypto improves +10.06%, Navier–Stokes +5.93%, Splay +1.00%, RegExp is neutral, and the largest regression is Richards at −3.36%, within the standing −5% per-suite gate. The change is accepted.

Acceptance: complete. Native calls with at most eight non-contiguous arguments perform no argument-vector heap allocation; larger calls preserve a correct general fallback; tests and A/B evidence are recorded.
