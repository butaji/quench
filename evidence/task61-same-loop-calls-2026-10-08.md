# Same-loop JS calls: M4 evidence

Host: Apple M4, macOS 26.5; Rust 1.99.0 / LLVM 23.1.2. Measurements use production-shaped binaries and CommonJS-shaped inputs. The candidate and control binaries use the same source except that the control forces `allow_inline_calls` false.

## Decision

Keep the guarded same-loop `PushFrame` path for ordinary same-program calls. It reduced retired instructions by 3.64% for `r=fn(i)` and 2.35% for `r=o.m(i)` against the forced-fallback control. The local-only loop rose 1.77% versus that control, so this path is retained for its measured call benefit and remains subject to the fixture-level Richards gate. Wall time improved in each of three paired runs for both call loops. RSS was flat.

## Paired instruction and timing results

| CJS loop | Candidate instructions/op | Fallback instructions/op | Delta | Candidate/control wall times (s, 10M ops; three pairs) |
|---|---:|---:|---:|---|
| `r=i` | 1037.33 | 1019.27 | +1.77% | mixed/noisy |
| `r=fn(i)` | 3544.92 | 3678.67 | −3.64% | 1.18/1.34, 1.28/1.33, 1.28/1.35 |
| `r=o.m(i)` | 5711.65 | 5848.80 | −2.35% | 1.88/2.03, 1.89/2.03, 1.90/2.06 |

The pre-trampoline control for `r=i` measured 1014.28 instructions/op, a +23.05/op local-loop tax for the candidate.

## Correctness and implementation boundary

The optimization only handles the guarded `Op::Call` case: same program and realm, general caller and callee, non-root, non-terminal call, and no direct eval. Other calls retain the existing path. The caller publishes the call-site pc before `PushFrame`, so GC and exceptions observe the caller's root map. Pending call state carries the saved call pc; normal return resumes at the caller's successor. The stack guard remains active through the child frame and is released on return or unwind.

A marker build confirmed that all 8/8 CJS local-call iterations entered the `PushFrame` path and returned the expected result. Focused direct-call and Node-oracle loops passed. Recursion boundary was unchanged: 509 nested calls succeeded and the next transition raised `RangeError`.

A clean `git archive` of HEAD `598b21591d4494fd8dd54b0aa58324161c08ac71` plus only the staged trampoline patch passed `cargo check --workspace --locked --offline --profile production`, built production `quench-node`, and passed the focused oracle against Node. The built quench-node SHA-256 was `a257396c6028ecc940309dbd01f4e28e3d79f58fccef8ec4c9330759adc937b3`; unrelated worktree changes were excluded.

## Binary identities

- Candidate: `08182ce7b4e707467184f835b9649d5916c8e28a52b33b09edd187c7d23da55c`
- Forced-fallback control: `356007523a8eb4ffad688c09acc8eb69d4578012d0070b6d1bbf12041025892a`
- Pre-trampoline control: `c8b9cfa760824f74b2815088c8ff83c4b0f11e770787eeb48f72948db0978d27`

## Broader check status

Focused call tests, Node oracle checks and recursion boundary checks passed. Test262 stages 1, 8, 11, 26, 36–38, 48–50, 58 and 62 passed. Stage 94 was 844/845; its lone `annexB/language/function-code/block-decl-func-skip-arguments.js` failure reproduced under local Node v26.10 script-mode VM, so it is recorded as an existing oracle mismatch. The post-merge request still requires the broader calls/generators/async/GC, Wasm and framework-core gate.
