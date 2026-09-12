# 355 — Reserved native call-activation stencil

Status: complete

Test whether reserving one already-built, cleared child activation at each populated
monomorphic `CallIcSite` can remove enough successful-call bookkeeping to justify the
remaining native-to-Rust-to-native transition. This is a general bytecode-call experiment:
there are no source identities, benchmark names, or execution-count selectors.

## Preflight and implementations

Task 181 measured inherited-property/direct-call populations of 8.56m/7.75m in Richards,
12.62m/10.19m in DeltaBlue, and 1.97m/1.82m in RayTrace over 500 ms. Task 336 showed that
retaining a cleared activation improves the complete aggregate by 3.84%. Tasks 337 and 353
showed that a small native guard or leaf which still calls the Rust dispatcher is neutral
or negative.

The first implementation copied the complete reserved-activation algorithm into every
direct-call `StencilInstance`. It recorded 1,764,056 native hits in a 100 ms Richards run,
but made the copied direct-call fragment roughly 900 bytes. The alternating 3 x 200 ms
complete-suite comparison was 2356.34 -> 2209.82 (-6.22%): Richards -3.09%, DeltaBlue
-2.23%, Crypto -5.82%, RayTrace -7.43%, Earley-Boyer -18.03%, RegExp -0.86%, Splay
-11.20%, and Navier-Stokes +0.37%. The result falsifies large per-site copying: executable
working-set growth overwhelms the saved bookkeeping.

The second implementation used the intended representation split. The copied call
connector returned to 112 bytes, while one immutable shared Rust `Kernel` performed
activation acquisition, binding initialization, child entry/completion, and result
transfer. A 100 ms Richards wiring run recorded 1,787,922 reserved-kernel hits from
1,847,192 attempts, proving that the candidate owned the intended dynamic path. All 128
release tests passed.

That split still regressed the alternating 3 x 200 ms complete suite:

| Suite | Baseline | Candidate | Change |
|---|---:|---:|---:|
| Richards | 935 | 902 | -3.53% |
| DeltaBlue | 933 | 879 | -5.79% |
| Crypto | 1827 | 1799 | -1.53% |
| RayTrace | 2014 | 1837 | -8.79% |
| Earley-Boyer | 2790 | 2461 | -11.79% |
| RegExp | 3741 | 3674 | -1.79% |
| Splay | 4049 | 3778 | -6.69% |
| Navier-Stokes | 7070 | 7084 | +0.20% |
| **Aggregate** | **2359.02** | **2240.09** | **-5.04%** |

Artifacts are in `reports/task355-reserved-native-call-ab-200ms-3/` and
`reports/task355-shared-call-kernel-ab-200ms-3/`.

## Decision

Reject and remove both runtime variants. Rebuilding after removal reproduces the accepted
binary bit-for-bit at SHA-256
`a02330525ba32d3d371027349f5029c75a5120c9f966644783c66053bafc9688`.

The result is stronger than “activation reuse does not help.” Task 336 proves reuse helps.
This task proves a shared kernel cannot compensate for the host ABI and recursive Rust
frame transition around every guest call. Deegen's own call operation is a CPS transfer to
a callee plus an explicit return continuation, and its IC stubs are not functions: they
operate on the surrounding JIT machine state and branch directly to continuations. Sources:
<https://arxiv.org/html/2411.11469>, Sections 4.4, 7.1, and Appendix A.3.5; V8's custom
register ABI and argument-adaptor removal provide the same practical boundary evidence:
<https://v8.dev/blog/csa> and <https://v8.dev/blog/adaptor-frame>.

The next call work is therefore only Task 146/181's guest stack: caller-reserved in-place
frame, pinned VM registers, direct callee transfer, and patched return/exception
continuations. Do not build another `execute_direct_call` wrapper, reserved Rust sidecar,
or large copied call stencil.
