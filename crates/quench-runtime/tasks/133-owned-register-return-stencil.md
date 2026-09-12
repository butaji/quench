# 133 — Ownership-transferring register return stencil

Status: complete

Make the terminal `Return` AOT stencil valid for every one-word JS value, including heap
strings, objects, functions, and regexps. A dynamic register file is uniquely owned by the
active call and is discarded immediately after return, so returning a register can move its
owner into `frame.result` and replace the source with `Undefined`. No reference-count helper
or semantic duplication is required.

This is a typed ownership morphism, not a relaxed copy guard:
`OwnedRegister × EmptyResult -> ClearedRegister × OwnedResult`. The singular Rust executor
already implements the equivalent move through ordinary `Value` ownership. The cooked AOT
handler must retain its slow edge only for an unexpectedly occupied result slot.

Do not apply the same rewrite to `LoadLocal,Return`: captured environments may outlive the
call, so a local slot cannot be moved without a proof that its frame does not escape.

Acceptance: cooked-stencil ownership test with an Rc-backed value, full release tests,
complete smoke, residual counters proving standalone `Return` misses disappear, and an
alternating full-suite A/B. Retain only if semantics and the standing regression floors pass.

## Result

The rustc/LLVM semantic source now emits two terminal templates selected from bytecode data:
`deegen_dyn_return` moves an owned register word into `frame.result` and clears the source;
`deegen_dyn_return_undefined` handles the no-value coproduct arm. Splitting these shapes is
required by the stencil extractor: expressing the optional source inside one handler made
LLVM duplicate the tail relocation, and the build-time one-continuation verifier correctly
rejected both attempted shapes. Each accepted handler has one syntactic next tail call.

Seventy-five release tests pass, including a cooked executable test that moves an Rc-backed
string, proves the source register is `Undefined`, and proves exactly one result owner remains.
The complete smoke passes in `reports/task133-owned-return-smoke.jsonl`. The residual profile
in `reports/task133-owned-return-residual.jsonl` reduces standalone `Return` generic entries
from 121,459 to zero. `LoadLocal,Return` remains generic for heap values, as required until
frame escape is proven.

The four-run, 500 ms alternating full-suite result in
`reports/task133-owned-return-full-ab-4/comparison.txt` improves 1159.72 to 1165.17 (+0.47%).
Every component remains above the -5% floor; DeltaBlue improves 2.34% and Splay 1.61%.
Final executable: `/tmp/deegen-task133-owned-return`, SHA-256
`9cd16751b8cc6a8ba9b7453f47f635aeac07a21f7ca663508dc22ddefaa73a24`.
