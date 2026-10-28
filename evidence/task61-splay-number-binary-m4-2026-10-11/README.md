# Number arithmetic fast path: M4 gate

This records the M4 arm64 macOS integration gate for `a75d344a2` on the
aligned trunk `32d8894d2`. The measured source was detached commit
`944efa4016d8d7bde91350fcd8108a0993701249` (trunk plus that one patch); the
corpus was clean at `64e1860c736c1b899708f4cd646721bc71e53d8b`. The production
candidate binary SHA-256 is
`b1222e3d7f636d9d9863f90eeb1d5b63b7ce0f1fb8833657cd0818ecc9a5cd62`; the
baseline binary is
`43de900ad8553ef12db8e372a061492f160c1bff7a9af25b166b1ba7a6620509`. The
fixed-work runner SHA-256 is
`f8e02485d1724f130fb318fdf26089896dcdc540c03131173464d92301765bfa`.

The all-eight fixed-work comparison completed with 11/11 valid and clean pairs
on each fixture and equal output. The resumable guard reports `passed`; clean
cycle intervals show no detected regression on any fixture, and RSS remains
within budget. Paired marginal effects are:

| Fixture | Cycles/run | 95% paired interval | Instructions/run | Max RSS |
| --- | ---: | ---: | ---: | ---: |
| Crypto | −1.413% | −4.624% to +1.593% | −8.251% | −0.668% |
| DeltaBlue | +0.022% | −0.748% to +0.460% | +0.014% | −0.703% |
| EarleyBoyer | +0.232% | −0.473% to +0.785% | +0.435% | −0.342% |
| NavierStokes | −2.499% | −4.484% to −0.412% | −4.416% | −0.461% |
| RayTrace | −0.139% | −0.768% to +0.245% | −0.488% | −1.079% |
| RegExp | +0.775% | −0.633% to +2.187% | +0.006% | −1.278% |
| Richards | −0.323% | −0.749% to +1.193% | −0.166% | −0.972% |
| Splay | −0.941% | −1.684% to +3.738% | −0.423% | −0.040% |

The Splay paired cycle point estimate is favorable but its interval crosses
zero. Treat this as a no-regression result, not a demonstrated speed gain. The
current Splay distance remains the last four-engine M4 measurement, 1.13x
speed and 2.09x RSS; this Quench-only gate does not refresh either bar.

The focused runtime test
`vm::tests::numeric_binary_fast_paths_preserve_number_edges_and_generic_coercion`
passed (1/1). A direct-dispatch probe applies all 12 numeric operators to 15
boundary-number pairs. Node, the aligned trunk binary, and the candidate
produced byte-identical output; the probe and outputs are retained here.

Raw report: [`fixed-work-all-eight-11.json`](fixed-work-all-eight-11.json).
Guard checkpoint and decision:
[`checkpoint.json`](checkpoint.json) and
[`checkpoint.decision.json`](checkpoint.decision.json). The runner's source
outputs are validated in the report; their per-run temporary materializations
were removed by the runner.
