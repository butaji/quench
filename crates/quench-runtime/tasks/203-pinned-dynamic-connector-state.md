# 203 — Pinned dynamic connector state and tag registers

Status: planned

Extend the dynamic stencil connector from only `frame/site` to a fixed, architecture-
described register context containing the highest-value immutable execution state:
frame/local base, current function/slow-data base, VM or heap base, continuation/site,
and the NaN-box tag constants whose materialization is otherwise repeated. Every leaf,
composite, IC arm, slow adapter, and kernel bridge consumes and produces this same typed
context; unused fields are explicit pass-through connector values.

Rust macros generate the identical ABI for all AOT handlers. The build must reject a
handler whose disassembly adds an unintended prologue/epilogue, spills pinned state on
the fast path, materializes a pinned constant again, or fails to end in the expected
tail branch. Task 158 layers temporary-value register allocation on top of these reserved
VM-state registers.

Use named per-architecture register budgets and tag constants. Do not hardcode physical
register numbers outside the architecture ABI module. Slow C/Rust calls may preserve or
reconstruct the context in one adapter; successful neighboring stencils must not cross a
C ABI boundary.

Acceptance: disassembly shows tag checks and frame/heap access using pinned registers;
ordinary numeric/property chains carry the context without reloads or spills; category
connector law tests cover leaves, frozen composites, inline IC arms, and kernel exits;
full V8v7 alternating A/B improves.

Primary sources: Deegen's register-pinning and tag-register optimization
<https://arxiv.org/abs/2411.11469>, Copy-and-Patch's continuation/pass-through register
protocol <https://arxiv.org/abs/2011.13127>, and V8 Liftoff's register-state snapshots
<https://v8.dev/blog/liftoff>.

Keep the per-function IC/site metadata base in this connector state. Sparkplug uses an
otherwise-unused compatible-frame slot to cache its feedback-vector pointer because most
operations need it. This VM already has one immutable `InlineSite` array per function;
the equivalent is to pin or frame-cache that base once and derive current metadata by
named offsets, instead of reloading unrelated owner structures inside each leaf. The site
array remains the single fact representation; do not create a second feedback table.

Additional source: <https://v8.dev/blog/sparkplug>.
