# Indirect-call SSA argument staging: rejected screen

The prototype keeps typed Wasm call arguments in Cranelift SSA values through the monomorphic indirect-call cache guard. It sends those values directly to the same-artifact native entry and writes the resolver's u64 argument buffer only on the slow path. Result and status slots, cache guards, and the dynamic resolver remain unchanged. The experiment was opt-in with `QUENCH_WASM_JIT_REFERENCE_SSA_ARGS=1`.

This was motivated by TSF's function 221 / PC 63 `call_indirect` site: 2,663,108 calls, of which later selective-tier diagnostics recorded 2,662,438 native same-artifact target hits. The target is function 231, `strlen`. Its 64-operator body has three loops and memory loads, so it did not meet the small acyclic callee-inlining screen. Argument staging was the narrower follow-up.

Both TSF arms used the same production Quench binary (SHA-256 `d7cea5d351c91b466ddd9411c36a169cc826064102e7a7597916498c5303d165`), benchmark binary (`42d0399b049c1182dcebc29e0fc3fff7251451110dc6b0035da05c3ab39bd4c3`), corpus revision `06785cf861ac44855f168cbbe829278c2802e6de`, and three rounds of `--jit --jit-wasm-selective`. The opt-in wrapper set the feature to 0 or 1. Both reports validated all rounds with equal output.

| Mode | Median TSF Score | Median peak RSS |
| --- | ---: | ---: |
| SSA staging off | 8.33 | 225.973 MiB |
| SSA staging on | 8.16 | 225.953 MiB |

The score median fell by 0.17 points (about 2.0%); the 0.020 MiB RSS reduction is below meaningful resolution. This short, separately-run screen gives no positive score signal, so the implementation was reverted without spending an 11-round run. It is a triage rejection, not a statistical qualification result.

`cargo check --locked -p quench-runtime --lib --tests` passed, and the TSF workload validated in both arms. A targeted unit-test build was attempted for `compiled_wasm_indirect_cache_passes_typed_ssa_arguments`; rustc was SIGKILLed while linking the full runtime test binary. The test and prototype are preserved in `prototype.patch`.

Raw reports: [`flag-off-3round.json`](flag-off-3round.json) and [`flag-on-3round.json`](flag-on-3round.json). The benchmark process saw identical wrapper hashes; both wrappers invoked the same Quench binary above and differed only in the feature environment value.
