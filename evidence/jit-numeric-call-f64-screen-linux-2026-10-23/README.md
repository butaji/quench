# Pre-unboxed JS JIT call screen — 2026-10-23

This rejected two-pair, one-round-per-invocation screen compared two production binaries on JetStream 3 `hash-map`, JIT enabled. Both binaries used the same merged source revision `cd2ec93d6003bde12c538f281b8811f2912626a2`, Rust 1.99.0 production profile, JetStream checkout `06785cf861ac44855f168cbbe829278c2802e6de`, Bun 1.4.2 and Node 24.19.0. Linux x86_64 host `acfbe6f17cde`, four logical CPUs, 4 CPU quota units and 16 GiB memory limit. Diagnostics were off. The runner validated output equality on every sample.

Control binary SHA-256: `aca61031784f3bd9849c8bfbfab86fc9513a1ead9a287a31449279a53625774a` (source tree clean). Candidate binary SHA-256: `0ac32c725ed564c95de28ed14c1ead91deb579170006b2fa91eb9d6aded09d82` (only the uncommitted numeric-argument helper diff). The candidate extracts the already-guarded `_findKeyEntry` key and entry key to `f64`, then calls the existing compiled artifact through a shared helper instead of rebuilding `Value` arguments and unboxing them again.

| Pair order | Control score / RSS MiB / wall s | Candidate score / RSS MiB / wall s | Candidate/control score / RSS / wall |
| --- | ---: | ---: | ---: |
| Candidate then control | 12.30 / 41.41 / 43.58 | 7.70 / 41.29 / 41.91 | 0.626 / 0.997 / 0.962 |
| Control then candidate | 12.62 / 38.49 / 41.05 | 7.71 / 38.76 / 52.63 | 0.611 / 1.007 / 1.282 |

One-round Score and wall time disagree substantially; the candidate's score is consistently lower by about 38%, while RSS is effectively unchanged and wall time changes direction. This does not support retaining the call-helper refactor. Revert it; no peer-win or causal speedup is claimed. Raw complete reports preserve per-engine results and host samples. This screen is only one fixture and does not qualify suite performance.
