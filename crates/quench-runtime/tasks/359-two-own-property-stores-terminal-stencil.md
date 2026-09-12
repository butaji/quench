# 359 — Two own-property stores plus terminal return stencil

Status: complete

Disposition: rejected and removed.

Consume Task 358's residual-block census by adding one coarse rustc/LLVM-cooked block
family for the normalized seven-op terminal form:

`LoadLocal(receiver₁) ; LoadLocal(value₁) ; SetStatic₁ ;`
`LoadLocal(receiver₂) ; LoadLocal(value₂) ; SetStatic₂ ; Return(undefined)`.

The form is the highest-frequency residual block in the current Earley-Boyer run at
567,373 entries in a 20 ms census. Selection uses only opcode and operand dataflow. It does
not inspect property names, function/source identities, suite identity, literal values, or
execution heat.

The closed handler validates both receiver shapes and cached slots before either effect,
rejects reference-counted sources or old slot values to the canonical slow block, writes
both tracing-object/immediate values directly, produces the implicit undefined result, and
tail-transfers to the shared function-exit morphism. This preserves all-or-nothing replay
on a cache miss and avoids materializing four dead virtual-register values.

All bytecode positions and instruction counts are named constants with compile-time
adjacency assertions in both runtime selection and AOT semantic source.

Acceptance:

- selector tests cover the exact form, operand mismatch, and observable-return rejection;
- an executable cooked-stencil test fills two monomorphic caches and proves both physical
  slots are updated without entering the patched no-op slow continuation;
- all release tests and all V8v7 suites pass;
- retain only if complete interleaved A/B clears the standing per-suite and aggregate
  floors.

## Result

Rejected and removed. All **132** release tests passed, and the selector added three
direct blocks to Earley-Boyer, but the targeted residual block still entered
`dyn_block_step_impl` **1,280,750** times in the 100 ms census. Its stores overwrite
reference-counted function/string values, so the ownership guard correctly rejected every
native attempt. A larger syntactic stencil cannot remove that semantic boundary.

Three alternating 200 ms complete-suite repetitions against Task 358 measured:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 946 | 944 | -0.21% |
| DeltaBlue | 943 | 960 | +1.80% |
| Crypto | 1848 | 1821 | -1.46% |
| RayTrace | 2003 | 2014 | +0.55% |
| Earley-Boyer | 2834 | 2801 | -1.16% |
| RegExp | 3747 | 3682 | -1.73% |
| Splay | 4091 | 4096 | +0.12% |
| Navier-Stokes | 7068 | 7047 | -0.30% |
| **Geometric aggregate** | **2375.47** | **2368.22** | **-0.31%** |

Artifacts are in `reports/task359-two-property-store-terminal-ab-200ms-3/`. The next
property-write work must make ownership and GC barriers an explicit composable effect so
the successful edge can update reference-bearing slots without returning to Rust.
