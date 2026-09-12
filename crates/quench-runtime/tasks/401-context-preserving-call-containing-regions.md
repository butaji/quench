# 401 — Context-preserving call-containing residual regions

Status: in_progress

Replace the dominant one-call generic Rust blocks with one coarse, typed quoted region:

```text
CallRegion<GammaIn,GammaOut> =
    Seq(Prefix<GammaIn,GammaCall>,
        Call<GammaCall,GammaReturn>,
        Continuation<GammaReturn,GammaOut>)
```

This supplies the surrounding cover required by Task 379. It does not reintroduce Task
362's rejected pattern of splitting every call into a tiny probe and a call-only fallback.
The region remains one category morphism even though final physical selection may use
copied stencils for the prefix/continuation, a patched call instance, and shared kernels for
cold semantic side exits.

## Static admission and canonical data

Start with maximal residual basic blocks containing exactly one ordinary user `Call`, where
every non-call bytecode already has a direct typed stencil or a normalized property IC
recipe. The selector is derived only from bytecode, CFG, immutable function/call recipes,
ownership/effect summaries, and named static budgets:

- `MAX_CALLS_PER_INITIAL_REGION`;
- `MAX_CALL_REGION_CONTEXT_VERSIONS`;
- `MAX_CALL_REGION_LIVE_VALUES`;
- `MAX_CALL_REGION_SIDE_EXITS`.

No runtime count chooses a region. A bounded coproduct prelinks exact noncapturing targets,
compatible general targets, and the total semantic kernel. Unpublished, polymorphic,
recursive, variadic, capturing, `arguments`-using, constructor, exception, and allocation
cases select an explicit compatible side exit until their context/effect obligations are
implemented.

The quoted representation stores a call boundary with live-in/live-out sets and an effect
token, not executable closures. Macroexpansion first performs local value numbering,
property-shape propagation, dead materialization removal, and context-version selection;
only the normalized expression is emitted. This preserves the Lisp
`quote -> macroexpand* -> eval once` discipline.

## Physical realization

The successful exact-target arm consumes `FunctionCallRecipe`, the stable activation, frame
layout, target entry, return continuation, argument slots, and destination slot as patch
data. It initializes the child prefix directly, transfers native-to-native, and returns to
the already linked continuation. The hot arm may not call `execute_direct_call`,
`dyn_block_step_impl`, or another Rust semantic helper.

If a compatible callee cover is unavailable, the whole original block remains one shared
semantic kernel; do not split around the call. Calls with known side effects invalidate the
appropriate Task 173 memory versions before the continuation. Shape/value facts survive
only when the call recipe proves the needed effect exclusion.

## Verification and gate

- Differential tests cover nested/recursive calls, arity padding, receiver/argument/result
  ownership, captured environments, exceptions, GC root tracing, polymorphism, and side
  exits at every boundary.
- Category tests prove prefix/call/continuation associativity and identity, and that every
  side-exit adapter reconstructs the same canonical frame state as the semantic kernel.
- Cooker audit and disassembly prove the successful arm has a matched native transfer and
  return continuation with no Rust helper.
- Diagnostics report statically admitted regions, selected covers, native call entries,
  semantic fallbacks, removed block seams, code bytes, and retained effects.
- A targeted screen must reduce the measured dominant Richards/Crypto residual shapes.
  Retain runtime code only after the complete nine-pair exact gate improves without a
  component-floor violation.

Primary design sources: interprocedural BBV
<https://arxiv.org/abs/1511.02956>, V8 Maglev's frame-state/known-information construction
<https://v8.dev/blog/maglev>, and Deegen's copy-and-patch/IC inline-slab pipeline
<https://arxiv.org/abs/2411.11469>.

## First executable slice: total direct cover and borrowed method-call composites

The first slice is always selected for a residual basic block containing exactly one
`Call` only when every surrounding opcode has direct stencil cover. Unsupported or
multi-call blocks remain one semantic kernel. This removes the rejected
kernel-prefix/call/kernel-suffix fragmentation from Tasks 181 and 362; there is no
environment switch, execution counter, or hotness threshold in selection.

An initial implementation exposed an important ownership seam. A function-valued
`GetStatic` correctly refused the ordinary copying property stencil because copying an
`Rc` word without retaining it is unsound. Its canonical slow edge then executed the
rest of the block, so linked direct-call stencils recorded zero runtime entries. Two
coarse rustc/LLVM-cooked templates now cover the normalized frontend forms:

```text
GetStatic(callee) ; Call
GetStatic(callee) ; LoadLocal(argument) ; Call
```

The property slot and local slot retain ownership while these templates borrow their raw
words into proven-dead temporary registers. The templates clear every borrow on success,
miss, and exception. A miss replays the original untouched sequence at its first
bytecode; success continues after the whole composite. Static liveness proves each
borrowed temporary is read only by the call and cannot alias its result.

The direct-call success hole now composes with the next stencil instead of pointing at
the semantic adapter. A named `DIRECT_CALL_EXCEPTION_LABEL` leads to a two-instruction
adapter that loads `resume_target` from the canonical frame and transfers to it; normal
return continues to use `return_target`. The cooker differential audit additionally
forced each coarse handler into one syntactic slow hole, preserving the single rollback
stud contract.

Tests prove a published monomorphic target produces an actual native entry, callee
identity mismatch replays canonical semantics, and an uncaught child exception takes the
dedicated resume edge. Both the 155-test release suite and forced-GC suite passed before
the later exception test was added; the complete suite is rerun for final acceptance.
Caught exceptions remain open because `PushHandler`/`PopHandler` currently prevent total
surrounding cover. The successful arm still calls `execute_direct_call`; therefore this
task remains in progress until the planned native guest-stack transfer removes that Rust
frame helper and handler control bytecodes receive compatible stencils.

The three-pair 200 ms screen at
`reports/task401-call-region-fused/quick-ab/comparison.txt` improves aggregate
**2406.31 -> 2448.27 (+1.74%)**. Richards improves **12.95%**, while every suite remains
above the configured component floor, justifying the exact gate against the accepted
Task 397 binary.

The nine-pair exact gate at
`reports/task401-call-region-fused/exact-vs-accepted/comparison.md` passes: aggregate
improves **2369.97 -> 2386.83 (+0.71%)**, with paired-bootstrap interval
**[+0.26%, +1.23%]**. Richards improves **11.10%** and every component clears the
configured floor. The accepted binary is
`/tmp/deegen-task401-call-region-fused-candidate`, SHA-256
`8fd8b77e7be10c968650c0bacb4cbc65a90497022fbf2a07297b56b877cada00`.

The final source state passes 156 release tests and 156 forced-GC tests. A stress-only
fixture failure was corrected by registering every function and object retained across
separate host calls as an explicit host root; it did not change the release binary hash.
This accepted slice is 23.87% of the 10000 target. Task 401 remains in progress for caught
handler cover and removal of the Rust frame helper from the successful arm.

## Post-gate reach correction

The accepted score is real, but the initial explanation attributed too much of it to guest
call execution. A native-path census performed after the exact gate shows that V8v7 enters
the coarse call-region stencils, yet `DIRECT_CALL_STATS` still records zero attempts/hits in
Richards. The temporary-owned-word guard was one blocker and has a general identity-preserving
reuse fix under test. The remaining blocker is structural: Richards methods are inherited,
while both cooked `GetStatic ; [LoadLocal] ; Call` composites currently consume only the own
property IC prefix. Their property guard therefore takes the semantic replay edge before the
direct-call callback is reached.

Consequently, do not claim that the exact +0.71% proves direct V8v7 guest-call execution. It
proves the combined Task 401/404 candidate improves; the guarded bitwise/shift coverage and
changed region layout are plausible contributors. The call design is proven only by synthetic
own-property tests until Task 368's bounded inherited `Presence` projection makes benchmark
attempt/hit counters nonzero and a new exact gate isolates its effect.

## Task 368 reach closure

Task 368's accepted one-level inherited `Presence` projection closes this specific reach
blocker. The Richards census now records 3,725,363 direct-call attempts and 3,502,225 hits,
so the coarse property/call morphism is demonstrably executing guest calls on the benchmark.
Its isolated nine-pair exact comparison improves **2352.14 -> 2401.58 (+2.10%)**, interval
**[+1.04%, +3.14%]**, with Richards **+11.61%** and no component-floor violation. Evidence
lives under `reports/task368-inherited-property/`; the accepted binary SHA-256 is
`06207d687c4ee53edde788a91950c9d49423322627c98f9ba9681ee73cb397ae`.

This resolves the post-gate reach correction but does not complete Task 401: successful
calls still cross the Rust frame helper, and caught-handler control flow still lacks total
surrounding stencil cover.

Task 407 tested a narrower prerequisite for the remaining native transfer: holding the
published reusable child activation in stable backing storage instead of moving its `Box`
through the call-site `RefCell`. After fixing a recursive reentrancy hazard, the exact
nine-pair result was +0.10% with interval [-0.73%, +0.95%], so the runtime change was
reverted. Task 401 must remove the Rust call boundary as a whole; merely changing activation
ownership around `execute_direct_call` is not a measurable substitute.
