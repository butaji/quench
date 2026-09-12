# 376 — Primary-source algorithm research, round thirty-seven

Status: complete

Research additional algorithms that fit the current VM rather than repeating obsolete
advice. The accepted runtime already has a word-sized tagged value, canonical shapes,
fixed property slots, a traced stable-object heap, bump allocation, call/property caches,
and real composed opcode stencils. Task 375's clean source-rebuild standalone exact score
is `2404.3308063773734`; its nine-pair acceptance result is +0.64% with a 95% interval of
[+0.10%, +1.01%]. The remaining problem is continuous native coverage and state
propagation, not the absence of a stencil container.

## Primary-source findings

1. Deegen's important unit is a semantic component with explicit slow paths, return
   continuations, pinned VM state, and IC effect arms—not a disconnected opcode body.
   Its generic IC criterion factors work into an idempotent key-to-state computation and
   a cheap input-plus-state effect. That supports Tasks 145/149/181 and rejects another
   helper-calling call leaf: <https://arxiv.org/abs/2411.11469>.
2. Lazy BBV reports eliminating 71% of type tests, typed shapes report eliminating 48%
   of type tests and reducing execution time 25%, and interprocedural BBV reports
   eliminating 94.3% of type-tag tests. These are external results, not projected local
   gains. They reinforce Tasks 144/152/177 only after total native cover exists:
   <https://arxiv.org/abs/1411.0352>, <https://arxiv.org/abs/1507.02437>, and
   <https://arxiv.org/abs/1511.02956>.
3. V8's Sparkplug and Lua 5.0 both show why static frame/register indices should survive
   lowering. Sparkplug directly serializes register bytecode into machine code, while
   Lua records upvalues by indices rather than resolving source names at every use. The
   current residual makes that specifically actionable as Task 378:
   <https://v8.dev/blog/sparkplug> and <https://www.lua.org/doc/jucs05.pdf>.
4. Arm documents that `BLR` pushes and `RET` pops the hardware return stack. The current
   tail-branch return continuation is semantically clean, but a direct call/return
   physical realization may predict repeated guest returns better. Task 379 is a bounded
   experiment, not a claim that Deegen's CPS design is wrong:
   <https://documentation-service.arm.com/static/649ac6d4df6cd61d528c2bf1>.
5. Per-opcode counts are insufficient when a block closes only after several missing
   capabilities are supplied. Frequent-itemset mining and budgeted maximum coverage
   provide useful search machinery, while compiler tree/DAG covering supplies the right
   cost vocabulary. Task 377 adapts them to the residual frontier:
   <https://rsrikant.com/papers/vldb94.pdf>,
   <https://doi.org/10.1016/S0020-0190(99)00031-9>, and
   <https://llvm.org/pubs/2008-06-LCTES-ISelUsingSSAGraphs.pdf>.
6. Partial evaluation remains the right build-time generator model, but it is already
   represented by Task 149. Weval's first-Futamura-projection result reinforces the
   existing binding-time worklist; it does not justify another runtime tier or a second
   semantic implementation: <https://cfallin.org/pubs/pldi2025_weval.pdf>.
7. V8's fast-property and element-kind designs reinforce existing Tasks 152/165/265.
   Immix-style line/block reuse is already represented by Task 300. RegExp one-pass,
   quick-check, and word-compare work is already represented by Task 87. No duplicate
   tasks were created for those mechanisms.

## Local residual interpretation

Task 373 counted 20,570,009 residual helper entries. The first unsupported operation was
`LoadName` for 9,150,871 entries, `Call` for 4,348,926, and `GetComputed` for 1,657,605.
The capability combinations matter more than the singleton counts: name plus call was
estimated to close 5,502,818 entries; computed access plus bitwise and numeric-unary cover
was estimated at 977,987. Task 375 proves the same phenomenon at smaller scale: equality
families closed whole blocks and survived the exact gate.

## Ranked next trials

1. Task 377: mechanize the residual-frontier hypergraph so the next family is selected by
   weighted whole-block closure per measured cost, including multi-family synergy.
2. Task 378: burn lexical depth/slot addresses into direct `LoadName`/`StoreName` leaves,
   using the existing bounded environment-chain view and layout guard.
3. Task 12: add the direct dense `GetComputed`/`SetComputed` leaves needed by maximal
   block composition; do not add another generic helper wrapper.
4. Tasks 309/316/348/366: let unsupported effects become explicit side edges so one
   dynamic operation does not demote an entire native prefix and suffix.
5. Task 379, then Tasks 145/181: compare an exact-arity native `BLR`/`RET` guest-call
   realization with the current explicit return-target branch, but only inside a fully
   native surrounding region.
6. Tasks 144/152/171/203: propagate type, shape, ownership, and register facts across the
   continuous region; then hoist/erase repeated guards and unbox loop-carried values.

Every trial remains general and always available from first execution. Measurements guide
engineering priority only; they never enter bytecode selection. All limits, costs, field
offsets, search depths, and ABI words must be named constants.
