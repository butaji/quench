# Interpreter source distillation

This document records mechanisms, not benchmark folklore. Each row comes from
the source revision corresponding as closely as possible to the locally
measured executable, and each proposed transfer must still pass rqj's exact
binary Score/RSS gate. JIT-generated guest code is out of scope.

## Revisions and modes

| competitor | measured executable | inspected source | interpreter mode |
|---|---|---|---|
| QuickJS | 2026-06-04 | [`04be246`](https://github.com/bellard/quickjs/tree/04be246001599f5995fa2f2d8c91a0f198d3f34c) | ordinary `qjs`; direct dispatch and short opcodes are compile-time defaults |
| Node/V8 | Node 26.9.0, V8 14.6.202.34-node.32 | Node [`b469d3f`](https://github.com/nodejs/node/tree/b469d3fd9401ecbd5de334f4b7043dd0286e4a7b/deps/v8) | `node --jitless`; Ignition handlers remain, runtime executable-memory allocation is disabled |
| Bun/JSC | Bun 1.3.14+d1632b291 | Bun tag [`0d9b296`](https://github.com/oven-sh/bun/tree/0d9b296af33f2b851fcbf4df3e9ec89751734ba4) pins WebKit [`5488984`](https://github.com/oven-sh/WebKit/tree/5488984d20e0dbfe4be2c3ba8fb18eb81a5e0e8b/Source/JavaScriptCore) | `JavaScriptCoreUseJIT=0`; JSC reports LLInt=true and JIT/Baseline/DFG/FTL=false |

The installed Bun build suffix is not a public Bun Git ref. The public 1.3.14
tag's explicit WebKit pin is therefore the reproducible source reference; it
is not claimed to be byte-identical provenance for the bottle.

## Mechanism table for Crypto

| mechanism | source evidence and why it is efficient | rqj state / transfer | expected effect and risk |
|---|---|---|---|
| Threaded dispatch | QuickJS's `JS_CallInternal` builds a 256-entry computed-goto table in [`quickjs.c`](https://github.com/bellard/quickjs/blob/04be246001599f5995fa2f2d8c91a0f198d3f34c/quickjs.c#L17746). JSC's [`dispatch`](https://github.com/oven-sh/WebKit/blob/5488984d20e0dbfe4be2c3ba8fb18eb81a5e0e8b/Source/JavaScriptCore/llint/LowLevelInterpreter.asm#L499) jumps through narrow/wide opcode maps. V8 tail-calls its dispatch-table entry in [`InterpreterAssembler::DispatchToBytecodeHandlerEntry`](https://github.com/nodejs/node/blob/b469d3fd9401ecbd5de334f4b7043dd0286e4a7b/deps/v8/src/interpreter/interpreter-assembler.cc#L1408). | rqj's exhaustive Rust match lets LLVM choose the machine layout; task 12 already rejected a function-handler table. Preserve the match unless assembly/profile evidence changes. | Potential dispatch reduction is large, but indirect handler calls damaged inlining previously. |
| Unified direct operands and destination | JSC's [`binaryOpCustomStore`](https://github.com/oven-sh/WebKit/blob/5488984d20e0dbfe4be2c3ba8fb18eb81a5e0e8b/Source/JavaScriptCore/llint/LowLevelInterpreter64.asm#L1194) reads `lhs`/`rhs`, performs the tagged fast path, and stores `dst` in one handler. [`loadConstantOrVariable`](https://github.com/oven-sh/WebKit/blob/5488984d20e0dbfe4be2c3ba8fb18eb81a5e0e8b/Source/JavaScriptCore/llint/LowLevelInterpreter64.asm#L585) gives every operand one frame-slot/constant representation. V8 uses an accumulator plus one frame-register input; QuickJS consumes adjacent stack values. | rqj Binary now has canonical register/local/constant/field operands and a register/local destination, but other operations still require transport. Extend the representation only where liveness proves eliminated instructions; broad mode tables failed RSS in tasks 131/153/182. | Fewer dispatches and smaller residual code. Extra hot decode arms can enlarge text/touched pages. |
| In-place local updates | QuickJS has direct [`OP_add_loc`](https://github.com/bellard/quickjs/blob/04be246001599f5995fa2f2d8c91a0f198d3f34c/quickjs.c#L19743), `OP_inc_loc`, and `OP_dec_loc`. JSC's destination-bearing operations write frame slots directly. | rqj marks `LoadLocal, IncDec, StoreLocal` but retains and fetches all three instructions. Physically compact the already-proved triple into one data-bearing instruction while preserving old/new result registers and virtual profile events (task 211). | Crypto executes about 124M triples. Compaction should reduce fetch/PC/code footprint; encoding complexity and postfix aliasing are the risks. |
| Tagged numeric fast path | QuickJS checks both integer tags once and uses checked 64-bit arithmetic in [`OP_add`](https://github.com/bellard/quickjs/blob/04be246001599f5995fa2f2d8c91a0f198d3f34c/quickjs.c#L19696). JSC performs tag branches, overflow-aware integer arithmetic, direct destination stores, then a cold slow path in `binaryOpCustomStore`. In V8 jitless, [`LoadFeedbackVectorOrUndefinedIfJitless`](https://github.com/nodejs/node/blob/b469d3fd9401ecbd5de334f4b7043dd0286e4a7b/deps/v8/src/interpreter/interpreter-assembler.h#L165) removes arithmetic feedback updates. | rqj's `Value::int_pair` similarly projects two tags once and `numeric_binary` handles the five measured integer rows. Operator retagging and duplicated operand-mode arms have already failed exact gates. | Current semantic core is close; gains are likelier from surrounding transport/dispatch than another arithmetic branch table. |
| Dense indexed access | QuickJS inlines integer-tag/class/bounds checks in [`OP_get_array_el`](https://github.com/bellard/quickjs/blob/04be246001599f5995fa2f2d8c91a0f198d3f34c/quickjs.c#L19434) and `OP_put_array_el`. JSC's [`op_get_by_val`](https://github.com/oven-sh/WebKit/blob/5488984d20e0dbfe4be2c3ba8fb18eb81a5e0e8b/Source/JavaScriptCore/llint/LowLevelInterpreter64.asm#L1811) specializes Int32/Contiguous/Double/ArrayStorage shapes in-handler; `putByValOp` also handles in-capacity growth before its slow edge. V8 routes keyed access through feedback-slot IC builtins. | rqj task 176 retains the tagged-int dense-array edge and task 197 feeds it local sources directly. Task 322 now forces only that dense wrapper inline while its property/coercion fallback remains cold and outlined. | Crypto has 125M dense hits and no misses. The precise hot/cold split improves Score without adding a storage representation or losing rqj's RSS lead. |
| Contiguous frames and calls | QuickJS allocates arguments, locals, operand stack, and var-ref pointers in one `alloca` block in `JS_CallInternal`; small call arities have `call0..3`. V8 frame registers are pointer-sized contiguous slots and `CallProperty0..2` avoid generic argument-list handling. JSC addresses virtual registers directly from `cfr` and has fixed call-frame headers. | rqj pools frames and passes all 3.14M six-argument numeric calls directly from caller registers, but retains separate local/register vectors within a frame. Task 195 found a single allocation slower; revisit only with a representation that also removes instruction transport. | Calls matter, but the measured argument-copy opportunity is exhausted. Stack allocation could lower allocator pressure but complicates escaping environments and recursive borrowing. |
| Native-local program counter | QuickJS keeps `pc` as a C local in `JS_CallInternal` and writes `sf->cur_pc` only before observable slow/call edges. JSC and V8 likewise keep interpreter PC/offset state in dedicated native interpreter state rather than round-tripping through a heap frame every opcode. | Task 308 keeps numeric-dispatch `pc` native-local. Task 383 applies the same rule to general dispatch and synchronizes at call, construction, GC, return, and error edges. | Task 383 improves Crypto by 0.74% in its 22-pair causal aggregate and improves Richards/DeltaBlue/Splay by 12.53%/3.17%/10.62%, with median RSS held. Stale PC at a reentrant/effect edge is the key invariant. |
| Deduplicated constants | QuickJS bytecode functions own indexed constant pools and reuse literal slots; V8/JSC also address constants by pool index rather than materializing every source occurrence independently. | Task 289 interns scalar constants by exact variant/number bits while preserving fresh contiguous constant-array runs. | Reduces residual-child materialization/fragmentation without changing JS identity; signed zero and array contiguity are tested. |
| Allocation and collection | QuickJS's small allocator uses 4-KiB size-class arenas (`js_malloc_block_sizes`, `__js_malloc`) for blocks through 512 bytes, reference counts values eagerly, and runs cycle removal under memory pressure. Apple's scalable allocator instead uses per-CPU tiny/small magazines. JSC uses segregated marked blocks and generational barriers; V8 uses a much larger generational heap. | rqj's tracing slots peak below 900 live cells in Crypto while about one thousand residual/runtime allocations occupy macOS zones. Tasks 356/357 bound the single-thread clean child to one magazine and request space-efficient reclamation, without changing the GC. | The policy removes 848 KiB of live physical footprint in paired `vmmap` snapshots and stabilizes RSS enough to admit faster handlers. It is macOS-specific; GC transplantation remains unjustified. |

## Current profile and decision

The retained aggregate build records 686,484,592 Binary operations,
402,359,361 semantic local loads, 336,404,364 semantic local stores,
125,133,879 indexed reads, 44,530,608 indexed writes, and 3,140,100 calls to
numeric-class functions. All indexed reads are integer dense hits; only 1,353
of roughly 630 million numeric binary attempts miss the fast path by type.
The hot `am3` residual is 45 instructions and still represents each local
update as three stored instructions even though the numeric loop dispatches it
as one marked operation.

The first transfer was physical, liveness-safe local-update compaction (task
211). It distilled QuickJS's direct local-update bytecodes and JSC's
destination-bearing frame operations without importing either engine's opcode
cross-product. Although `am3` shrank from 45 to 39 instructions, exact Crypto
regressed from **1710 / 3,440,640 bytes** to **1688 / 3,522,560**. The compact
handler had to absorb generic non-integer coercion that the retained marker
leaves in separate ordinary instructions. The implementation was removed.
This narrows the lesson: bytecode density is valuable only when slow semantic
edges remain out of the hot handler, as they do in QuickJS and LLInt.

Task 214 then outlined both the integer executor and cold coercion edge. Score
rose from 1686 to 1747, validating the upstream layout lesson, but RSS rose
from 3,506,176 to 3,571,712 bytes. Smaller validation and fallback variants
still touched an extra page. The compact carrier is therefore closed: the next
transfer must replace or shrink existing code instead of adding a parallel
representation, even one only hundreds of text bytes larger.

Task 308 then transferred QuickJS's native-local `pc` discipline directly.
The isolated build improved Crypto Score by 31.1% but touched 128 KiB more
RSS. Task 289 supplied the complementary representation reduction without
changing dispatch: exact scalar constants share one residual slot while
constant-array payloads remain contiguous. The retained pair (task 316)
measures **2322 / 3,178,496 bytes** in the 11-run tournament, giving rqj the
lowest RSS of all four engines. Bun/JSC's **3703** remains the active Score
target.

Task 304 then removed the boxed `Vec` header from fixed-size captured
environments, using `Box<[Value]>` as the single representation. This lowers
the 11-run Crypto checkpoint to **3,129,344 bytes** while slightly improving
Score to **2377**, creating measured page budget for subsequent transport
experiments.

Task 322 used that budget to transfer the exact dense-index boundary found in
QuickJS and JSC: the integer/class/bounds wrapper is always inlined, while the
complete property/coercion implementation stays `inline(never)`. The 11-run
Crypto checkpoint is now **2394 / 3,129,344 bytes**. The unchanged RSS and
the gains on all three cumulative workload screens show that this is a
general interpreter hot/cold decision, not benchmark-shaped guest logic.

Task 332 then sampled the native task-322 image and attributed 79.7% of
steady-state stacks to the inlined numeric loop, concentrated at fixed-record
fetch/decode and its integer core. Narrowing the whole instruction to 10 bytes
failed (task 333), but filling its existing alignment byte with a u16 opcode
removed one front decode load. That change alone gained Score but lost six RSS
pages (task 334). Composing it with PGO opt-level 2 (task 335) shrinks `__TEXT`
by 32 KiB and wins the paired Crypto gate at **2408 / 3,145,728 bytes** versus
task 322's **2360 / 3,162,112**. The current throttled four-engine tournament
still leaves Score open at **1682** versus Bun/JSC's **2797**, while rqj keeps
the lowest RSS.

Task 336 repeated native sampling on that exact opt-level-2 image: 82.3% of
resolved stacks remain in numeric dispatch, while GC, native calls, and memset
are collectively negligible. Tasks 337–340 then separated four plausible
transfers from their source-level appearance. Expanding two profiled operand
modes gains 4.3% Score but costs 80 KiB RSS; moving `imm` to offset zero costs
112 KiB; borrowing the instruction rather than copying it loses 4.2% Score;
and forcing adjacent `b/c` fields through one 32-bit load is Score-neutral but
costs 80 KiB. All four are removed. LLVM already scalarizes the copied record,
and QuickJS/JSC's handler-local operand loading cannot be reproduced by merely
changing Rust aggregate syntax or adding extraction paths. The next transfer
must delete a semantic transport operation or shrink the whole hot handler;
local load-count arguments require emitted-code evidence and a resident-page
gate, not just source-level instruction counting.

Tasks 342–345 then made that gate causal. Timed V8 harness training generated
different PGO edge counts and an 80 KiB RSS spread from identical source;
clockless fixed-iteration training now produces byte-identical code/data across
rebuilds. Under that control, carrying update destination/direction in the
already-marked `LoadLocal` removes two residual fetches and improves Crypto by
7.6% while reducing RSS by 160 KiB. The original following `IncDec` and
`StoreLocal` remain the sole cold semantic fallback. A 2× DeltaBlue training
weight damaged Crypto without improving Delta and was rejected; the balanced
432/250/1012/61 corpus is retained. This is the useful QuickJS `add_loc` lesson
in rqj's representation: residualize hot metadata into an existing carrier,
but do not introduce a new opcode or absorb the cold coercion edge.

Tasks 346–348 resampled that retained image and tested the remaining inline
integer selector directly. Native sampling attributes 79.5% of steady-state
Crypto stacks to numeric dispatch, with instruction decode and operator
selection the hottest resolvable PCs; generic fallback is only 1.0%. Yet the
operator table is not profitably extended or pruned by frequency: adding the
27.4M-hit left-shift row loses 4.9% Score and 64 KiB RSS, while removing the
31.6M-hit `>=` row loses 3.3% and 176 KiB. Both builds change `__text` by at
most 20 bytes. The current five-row table is therefore a measured LLVM/PGO
layout fixed point. The transferable QuickJS/JSC lesson remains structural
(separate handlers with cold semantic edges), not “put every frequent integer
operator into one larger Rust match.”

Task 349 retested QuickJS's register-resident `var_buf`/`sp` discipline after
the deterministic-PGO and update-metadata changes. rqj cached only stable
register/local allocation bases and excluded environment-promoting functions
from the specialized loop. Although native text shrank by 412 bytes, Crypto
lost 0.9% Score and six resident pages. With `pc`, frame index, code base, and
instruction stride already held in native registers, forcing two additional
long-lived bases now creates harmful register pressure. This transfer remains
sound but is not compositional with the current ARM64 loop; future transport
work must shorten live ranges or delete state rather than pin more of it.

Task 350 shortened those live ranges to one operation by macro-generating only
the measured register/register and local/local projections. This recovers a
real 2.1% Crypto Score gain and removes 9.2 KiB of native text, confirming the
handler-local operand lesson from QuickJS/JSC. It still adds four median
resident pages and finishes 32 KiB above QuickJS, so it is removed under the
two-axis gate. The result is now a precise composition constraint: this
projector becomes admissible only after an independent reduction of at least
two 16-KiB pages, with its throughput gain revalidated in the composition.

Tasks 351–353 show why allocator policy cannot be admitted from a short paired
screen. `vmmap` assigns the projector's entire footprint delta to SMALL/TINY
fragmentation even though it has fewer live allocations and less resident
text. Disabling macOS nano metadata appeared to recover two pages and composed
with the projector at first, but the 11-run four-engine schedule measured rqj
at 3,227,648 bytes versus QuickJS at 3,178,496. The same build scored 2,561,
ahead of QuickJS and Node but below Bun's 3,742. Exact-capacity residual
decoding likewise eliminated every measured decoder realloc yet added three
pages. Malloc page geometry is schedule-sensitive; only the full rotating
tournament is authoritative for memory-policy compositions.

Task 354 measured all 158 Crypto residual functions before considering inline
code storage. Only 14 fit four instructions and 53 exceed 32, so embedding a
small buffer in every function would enlarge the cold representation and add
a choice to every fetch for too few removed allocations. No code was added.

Task 355 then transferred QuickJS/JSC's operation-specific arithmetic handler
structure without multiplying operand modes. Residual add/multiply handlers
improved Crypto by 7.5%, but macOS's default magazines turned the changed
allocation schedule into a 144-KiB RSS loss; add-only and multiply-only each
still lost 32 KiB. The handlers were correctly rejected in isolation.

Tasks 356/357 resolve that composition constraint at the actual host edge.
Apple's libmalloc source shows that the scalable zone keeps per-CPU magazines,
a contention optimization rqj's single-thread clean child cannot use. Setting
`MallocMaxMagazines=1` and `MallocSpaceEfficient=1` only before residual `exec`
reduces paired live footprint from 2,176 to 1,328 KiB, mostly by making 608 KiB
of small-zone dirty pages reclaimable. With identical policy on both artifacts,
the two operator rows improve exact Crypto by 6.1% and save one page. The final
11-run tournament is rqj **2684 / 2,736,128 bytes**, QuickJS **2534 /
3,194,880**, Node jitless **2563 / 45,252,608**, and Bun/JSC no-JIT **3761 /
55,934,976**. rqj now leads QuickJS and Node on both Crypto axes and keeps the
lowest RSS; Bun's Score remains the active target.

Task 375 tests the representation lesson directly without adopting a variable
byte stream: QuickJS uses one-byte opcodes with format-sized operands, V8 uses
byte opcodes plus operand-scale prefixes, and JSC generates narrow/wide opcode
maps. rqj instead packs its already-specialized fixed schema into one validated
64-bit word. ARM64 now fetches an instruction with one power-of-two indexed
`ldr`; the interpreter retains its inlined Rust match and a single canonical
stream. The clean causal gate gains 6.57% Crypto Score and saves 1.73 pages;
the tournament reaches **2850 / 2,719,744**. This isolates the transferable
property as compact one-fetch transport, not any upstream dispatch machinery.

Task 376 isolated the next transport layer. At the pinned revisions, QuickJS
dispatches from a local `uint8_t *pc` and saves it to the frame only at
observable helper/call edges; V8 Ignition advances an SSA bytecode offset and
passes it through handler tail-dispatch, reloading after effects when needed;
JSC LLInt advances a dedicated `PC` relative to `PB` before narrow/wide
opcode-map dispatch. The common mechanism is native-resident bytecode position,
not a particular pointer, opcode map, or handler ABI. rqj tested only a pointer
into its immutable packed stream while retaining the inlined Rust match. ARM64
did improve mechanically to a post-indexed `ldr`, but the decisive clean gate
regressed Score by 0.1881% (95% interval -0.3634% to -0.0124%) and left RSS
inconclusive. The cursor and supporting validation were removed. The important
boundary is now empirical: native PC residence transfers, but substituting an
equivalent address form does not; a follow-up must eliminate dispatch or
semantic transport. Exact citations remain in
`tasks/evidence/376-cursor-runtime-source-extract.md`.

Task 383 then transferred the semantic rule without repeating task 376's
rejected address-form experiment. The general interpreter keeps its integer
instruction position in a native local and publishes it only at observable
call, construction, GC, return, and error edges. Optimized ARM64 confirms that
the common latch no longer loads or stores `Frame.pc`. The 22-pair causal
aggregate improves Crypto Score by 0.7377% while holding its RSS median; a
strict 21-rotation external aggregate reaches **2870 / 2,703,360**, retaining
the lowest RSS but remaining below Bun/JSC's **3755** Score. Richards,
DeltaBlue, and Splay improve by 12.53%, 3.17%, and 10.62% respectively with
unchanged median RSS. This broad effect is the expected signature of removing
general interpreter state transport, and is evidence against benchmark-shaped
specialization.

Task 390 verifies the next native attribution directly from the counted call
tree. A generic parser derives all 2,273 exclusive samples and requires exact
agreement with macOS `sample`'s independent symbol summary. Of 310 samples in
general dispatch, 293 resolve to the relative jump-table load; numeric
dispatch's 1,825 samples are dominated by its packed-word fetch and matching
jump-table load. This is not evidence for another semantic fusion. It confirms
that rqj's current machine loop already has the direct-dispatch shape shared
by QuickJS computed-goto, Ignition tail-dispatch, and LLInt opcode maps. The
remaining source-level alternatives are not untested: indirect Rust handlers
and a pointer cursor were rejected in Tasks 12 and 376. No production change
is justified without a changed representation or new hardware-cost evidence.

Task 391 obtains that new hardware evidence from the Apple M4 PMU. The L1D
sampling modes distribute misses across method-cache maintenance, allocation,
copies, and ordinary VM work; packed instruction fetch is not a concentrated
cache-miss site. In contrast, 288 of 294 discarded-indirect samples resolve to
general dispatch's final `br x9`, versus only three in numeric dispatch. The
source distillation remains precise: QuickJS, Ignition, and LLInt show that an
indirect opcode edge is expected, not that every opcode must reach it. A
bounded direct predispatch can only be considered after profiling the missing
General/Numeric × opcode cross product, with the existing jump table retained
as the single complete semantic representation.

Task 392 supplies that cross product as derived state. General dispatch
executes 122.50M physical targets; `LoadLocal` alone is 35.66M (29.11%), ahead
of `Binary` at 20.99M and `GetField` at 10.96M. The report explicitly keeps
compiler-elided virtual local events out of physical target frequency. This
supports one narrow layout test: branch directly to the canonical `LoadLocal`
handler before falling through to the existing jump table. The upstream
lesson is handler locality and predicted dispatch, not copying computed-goto
or LLInt assembly into Rust; all semantics and every other opcode retain the
single match-generated representation.

Tasks 393–400 then exhaust the direct local-load transfer under exact gates.
A General predispatch branch and handler-local runtime chaining both improve
Crypto but regress Richards. A declarative residual pair removes that runtime
test but crosses a resident code page. Finally, validating serialized
`LoadLocal` sources once and directly indexing the frame shrinks the ARM64
handler from 80 to 56 bytes. A post-decode data fold limits complete PGO growth
to 20 bytes and improves Crypto by 0.9038% at equal median RSS, but two clean
Richards blocks aggregate to a supported 0.5543% regression. This refines the
pinned QuickJS comparison: its direct `var_buf[idx]` access lives in a stable
hand-laid interpreter and its documentation forbids untrusted bytecode;
rqj must validate its serialized residual and also prove LLVM's resulting
whole-loop placement across workloads. The unsafe edge remains rejected until
both properties compose.
