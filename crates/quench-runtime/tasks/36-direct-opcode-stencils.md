# 36 — Direct rustc/LLVM opcode stencils

Status: in_progress

Replace the current block trampoline into `dyn_block_step` with composed rustc/LLVM-produced handlers that execute common bytecodes directly. The internal ABI carries frame/register/constant/next-instruction state in fixed locations. Each handler ends in a tail transfer patched to its successor; branches patch symbolic bytecode targets. Kernels remain only for semantic slow paths such as coercion, allocation, calls, misses, and exceptions.

Start with the dominant closed subset: local/literal load and store, move, numeric guard/arithmetic/comparison, unconditional and conditional branches, static shape-IC hits, and return. Lowering must select templates from the canonical opcode table; no source-pattern or benchmark-specific selection.

Evidence for priority: V8v7 structured runs report tens to hundreds of millions of `KernelExit` executions and zero `opcode_inline_entries`; sampling previously identified `dyn_block_step` as dominant. Shapes improve lookup inside the helper but cannot remove the per-op Rust dispatch ceiling.

Acceptance: counters show inline entries for supported opcodes and kernel exits only on real slow paths; the same semantic tests pass; composed machine code contains resolved direct successor/branch links; A/B results demonstrate a material aggregate gain.

Current state: Rust macros generate direct literal/local/move/arithmetic/comparison handlers. The extractor records and verifies both `next` and `slow` tail relocations, the linker resolves both kinds, and a fixed frame/site ABI is wired. A coarser `deegen_dyn_run` handler can execute a numeric prefix and rejoin the canonical block slow path.

Rejected experiment: per-op slow exits made Richards/DeltaBlue slower and caused Crypto/RayTrace timeouts. A block-level fallback fixed the exit cascade, but a generic 664-byte `dyn_run` still reduced Richards to roughly 205 and RayTrace exceeded 30 seconds. Selection is therefore opt-in through `DEEGEN_DIRECT_OPCODE_STENCILS=1`; the accepted default remains the faster block stencil. Next work must use frozen, guard-typed block/superinstruction templates rather than a generic opcode loop.

Current CFG experiment: direct `Jump` and `JumpIfFalse` templates now expose symbolic taken-edge relocations alongside sequential and slow connectors. The linker can therefore close complete numeric/local blocks without routing ordinary loop back-edges through Rust. This experiment is also still opt-in: a 20 ms diagnostic run measured Richards 311, Crypto 210, and Navier–Stokes 262 versus the immediately preceding default samples 305, 252, and 358. Correct control-flow composition alone does not offset the 44–140 byte primitive bodies and repeated descriptor/frame loads. The result narrows the next design to coarse, statically selected block templates.

Runtime-frequency evidence from [[83-runtime-block-frequency-profile]] confirmed that the previous default was still a Rust `dyn_block_step` loop, not seamless native execution: it recorded zero direct inline entries while tens of millions of bytecodes executed in the block helper. The highest-frequency reusable block shapes are numeric local/literal and local/name comparisons, counted in millions of entries; Crypto and Navier–Stokes additionally spend over a million entries each in large numeric/array blocks. [[84-dead-result-condition-stencils]] closed the first of those regions, passed the alternating A/B gate at +2.22% aggregate, and promoted coarse direct stencils to the default. `DEEGEN_BLOCK_KERNEL_ONLY=1` is now a measurement escape hatch; unmatched blocks and semantic misses still use the generic kernel.

Research round six changes the implementation order. The current general copied block
still enters `deegen_dyn_run`, decodes `InlineSite`, and performs an opcode `match`; it
therefore removes neither dispatch nor operand decoding. Before adding more source-level
regions, complete the build-time derivative catalog (Task 149), direct operand and
successor relocation forms (Task 79), pinned connector state (Task 203), and
register-resident planning (Task 158). A successful fast-path leaf must contain the
native semantics with burned-in operands and direct control transfer: it must not call
`deegen_dyn_run`, `dyn_block_step_impl`, or another generic opcode executor. Use the
generic executor only as a semantic slow kernel reached by a guard failure.

Task 283 adds one more valid coarse leaf: the normalized
`LoadLocal; GetStatic; StoreLocal; Jump` block is a single 140-byte cooked own-property
stencil. It checks the monomorphic shape/slot IC, transfers an immediate slot value
directly to its observable local, and branches to the patched CFG successor without
entering either generic executor. Heap ownership and semantic misses use the canonical
slow edge. The full A/B is +0.18%; Task 36 remains in progress because this covers only
19 compiled blocks, not the ordinary property/call vocabulary.

Task 284 confirms that merely inserting those leaves into generic semantic ranges is
not sufficient. Although a lawful flat tiler selected 107 additional property atoms,
its selected-suite A/B regressed 1.34% because each isolated atom introduced extra
native/Rust/native range boundaries. Further coverage must close adjacent atoms into
one cooked block/region or remove the generic range boundary itself; node-count coverage
is not a valid cost model.

Task 285 adds two more exact coarse leaves for complete `instanceof` condition blocks.
The expensive name lookup and complete prototype-membership cache remain one shared
kernel; 108/112-byte copied stencils consume its one-bit result and branch directly,
without materializing dead bytecode registers. The complete A/B is +0.10%, driven by
Earley-Boyer +2.60%. This validates kernel-produced predicate plus stencil-consumed
control as a memory-efficient composition, but Task 36 remains open: generic property,
call, allocation, and mixed blocks still dominate residual execution.

Research round twenty-two isolates another reason direct coverage can remain slow:
control relocations are closed, but ordinary leaf operands are frequently decoded from
`InlineSite` at runtime. Task 316 owns the generalized typed-hole vocabulary and linker
encoders for register/local byte offsets, literals, IC data, and direct targets. A leaf
does not count as Deegen-style direct execution merely because its successor is patched;
its disassembly must also prove that closed operands were burned into the instance or
its relocation-closed local data instead of repeatedly loading a descriptor.

Task 358 repairs the AOT ownership predicate after ordinary objects moved to the tracing
heap. Cooked moves, property transfers, destination overwrites, and terminal forms now
keep `ObjectHandle` values on their native edge; only strings, functions, and regexps take
the reference-counted slow edge. The complete aggregate improves 0.30%. A contemporaneous
Richards profile still attributes 55.01% of samples to `dyn_block_step_impl`, so this task
remains open and coarse mixed property/call blocks remain the dominant missing leaves.

Task 373 finally wires the primitive catalog as a maximal, all-or-nothing basic-block fold
and adds guarded own-property get/set plus boolean-not leaves. Exact native coverage rises
from 903 blocks / 2,299 opcodes to 1,755 / 5,533, and the required nine-pair exact gate
measures +2.42% [95% CI +2.05%, +2.86%]. This validates categorical composition as
executable infrastructure, not merely metadata. Task 36 remains in progress because
unsupported names, computed properties, allocation, construction, and most calls still
route whole residual blocks through `dyn_block_step_impl`; the next slice must close those
effect boundaries without reintroducing isolated native/Rust/native atoms.

Task 375 adds macro-generated strict/loose equality and inequality leaves to that same
fold. The nine-pair exact gate measures +0.64% with a 95% interval of [+0.10%, +1.01%],
and direct block/opcode counts rise across the equality-heavy suites. The next direct
families should be chosen from Task 377's weighted residual-frontier combinations. Current
evidence favors direct lexical-address leaves (Task 378) and dense computed access (Task
12), followed by call work only when its surrounding region remains native.
