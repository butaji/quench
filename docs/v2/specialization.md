# Specialization architecture: Futamura projections and Ershov's mixed computation

Status: the binding contract for Quench's JIT and AOT. Work order and status live in Linear
([TOD-1232](https://linear.app/todoxapp/issue/TOD-1232) and its sub-issues). This page records the
design and the reproducible measurements that shaped it.

## Roles

Quench has one interpreter and builds every native tier by specializing it.

| Futamura role | Quench artifact |
| --- | --- |
| Interpreter `int` | The shared semantic operations: `Vm::resolve_step_operation` per opcode and the staged operation families in `vm/operations.rs`. |
| Static input | One function of a `ResidualProgram`: OXC lowers source to residual bytecode, which fixes opcodes, operands, constants, jump targets and function boundaries. `Engine::specialize` already names that step. |
| Division | Ershov's binding-time division of every instruction input, derived from the `opcodes!` table (`specialize::BindingTime`). |
| Specializer `mix` | `specialize::Specializer`: it takes a static input and residualizes the interpreter for it. |
| Generating extension | `specialize::GeneratingExtension`: for one instruction and its static operands, it produces that instruction's residual code. A program's compiler is the generating extension applied across the function. |

**First projection.** `mix(int, program)` is native code for one program. The AOT object
writer, the eager native publisher and the JIT regions are all first-projection specializers.
They differ in only two respects:

- **Static facts.** Each specializer folds a different set: its division.
- **Publication.** The residual code becomes a linked object, live code memory, or copied stencils.

**Second projection.** `mix(mix, int)` is a compiler. Quench never self-applies a specializer
at run time. Two mechanisms produce the second projection instead:

- **Build-time generating extensions.** Rust monomorphization specializes the interpreter to
  each opcode, as in `native_js_operation_entry::<H, OPCODE>`. Macros derive the opcode tables.
  This is the cogen step, performed by rustc.
- **Stencils at process start.** Copy-and-patch compiles the residual shape of each generating
  extension once into a stencil. A stencil is machine code whose holes are relocations against
  the static inputs. Compiling a program is then copying stencils and patching holes, with no
  IR and no per-function Cranelift run (`vm/native_stencil.rs`).

**Third projection.** `mix(mix, mix)` is a compiler generator. In Quench this is the macro
system and is outside the runtime.

## Ershov's division

Each instruction input has one binding time. The `opcodes!` table is the only authority for it.

| Binding time | Meaning | Examples | Who may fold it |
| --- | --- | --- | --- |
| Static | Fixed by the residual program | Immediates, constant indices, jump targets, flags, operators, function indices, counts | Every specializer, AOT included |
| Location | Fixed storage holding a dynamic value | Register numbers, register windows, local slots | Every specializer folds the address; the contents stay dynamic |
| Site | A static index naming mutable feedback | Inline-cache, method, template and RegExp sites | The online (JIT) specializer only, behind a guard with exact-PC exit |
| Dynamic | Known only to the running activation | Register contents and heap state | Nobody without a guard |

Rules:

1. **AOT folds build-time facts only.** An AOT image never bakes site contents, so it needs no
   training run.
2. **The JIT folds site contents behind a guard.** A failed guard resumes the interpreter at the
   exact failing PC with its frame intact.
3. **A fact folded without a guard must be static.** Representation facts about dynamic values,
   such as "this register holds an int32", are guards, never folds.
4. **Static facts are never recomputed at run time.** A residual that re-derives a static fact
   is a binding-time violation. Re-deriving the program, the function, the decoded instruction
   or the opcode on every call is one example. Such violations are performance bugs and are
   removed in plan order.

## Specializers in the tree

| Specializer | Division folds | Residual | Publication |
| --- | --- | --- | --- |
| Call-threaded (`native_graph`, `native_stencil`) | Static and Location: dispatch, PC, operand records, successor | One semantic-entry call per instruction | Linked object (AOT), Cranelift or stencils in live memory (eager) |
| Guarded regions (`compiled.rs`, `compiled/osr_loop.rs`) | Static, Location and guarded Site/representation facts | Cranelift SSA with unboxed values, guards and exact-PC exits | Live memory (JIT) |
| Wasm (`compiled.rs`) | Static: Wasm types fix representations | Cranelift SSA | Live memory |

## Measured screen: two publications of the call-threaded residual

These figures come from `examples/native_publication.rs` on Linux x86-64 with 4 vCPUs and
15 GiB of memory, release build at `39d7c0d`, 11 rounds, medians. The workloads were generated
scratch inputs, not checked in. They are a screen, not a qualification.

| Workload | Interpreter run | Cranelift compile | Stencil compile | Cranelift run | Stencil run | Cranelift mapped | Stencil mapped |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Object loop, 5 functions | 175 ms | 4.3 ms | 0.20 ms | 249 ms | 267 ms | 24 KiB | 28 KiB |
| 3001 small functions | 30 ms | 3815 ms | 64 ms | 26 ms | 28 ms | 14.6 MiB | 24.9 MiB |

What the screen shows:

- **Stencils remove compile cost.** On the large program, compile time falls by a factor of 60.
- **Stencils need to shrink.** Each step stencil carries a frame and three callee-saved spills,
  so it maps more code than Cranelift's graph.
- **The call-threaded residual is slower than the interpreter** on the object loop, whichever
  publication is used. Folding dispatch alone is not enough while each semantic callout
  re-derives static facts (rule 4).

## Verification on V8-v7 (JIT mode)

V8-v7 under `--jit` is the acceptance workload for this design. The target is at least 1.20
times the best of Node and Bun on Score, at no more than 0.80 times their peak RSS. The rules
are in `jit-design.md`.

Screen of branch head `726919b8e`, which includes the `v2-jit` OSR and collection commits:
three rounds per engine through `quench-bench/v8-v7-rss.cjs`, Linux x86-64, 4 vCPUs,
Node 22.23.4, Bun 1.4.4, release `quench-node`, medians. Ratios compare Quench `--jit` with
the better JIT-mode peer: higher is better for Score, lower is better for peak RSS.

| Fixture | Quench `--jit` | Node | Bun | Score ratio | Peak RSS ratio |
| --- | ---: | ---: | ---: | ---: | ---: |
| Richards | 1683 | 35108 | 38398 | 0.044 | 0.298 |
| DeltaBlue | 1678 | 78938 | 48002 | 0.021 | 0.195 |
| Crypto | 16926 | 41408 | 48167 | 0.351 | 0.326 |
| RayTrace | 5382 | 55666 | 95977 | 0.056 | 0.172 |
| EarleyBoyer | 7041 | 59777 | 47222 | 0.118 | 0.176 |
| RegExp | 3857 | 7516 | 12845 | 0.300 | 0.301 |
| Splay | 6545 | 10395 | 22483 | 0.291 | 0.428 |
| NavierStokes | 49522 | 39771 | 31188 | 1.245 | 0.527 |

Peak RSS meets the bar against JIT-mode peers on all eight fixtures. Score meets it on
NavierStokes only. Splay's RSS rose from 34 to 79 MiB with `v2-jit`'s collection-headroom
commit, which traded memory for Score. It remains below both JIT peers but above Bun without
JIT, so the board's stricter all-modes rule marks it lost.

The interpreters, compared in the same screen with JIT off, show where the gap lies:

| Fixture | Quench no-JIT | Node `--jitless` | Bun no-JIT | Node JIT |
| --- | ---: | ---: | ---: | ---: |
| Richards | 1936 | 1190 | 1402 | 35108 |
| DeltaBlue | 1881 | 1365 | 1120 | 78938 |
| Crypto | 3121 | 1188 | 1684 | 41408 |
| RayTrace | 5230 | 3520 | 2547 | 55666 |
| EarleyBoyer | 6935 | 5508 | 4533 | 59777 |
| RegExp | 3855 | 3234 | 729 | 7516 |
| Splay | 7278 | 5018 | 3973 | 10395 |
| NavierStokes | 3240 | 1681 | 2545 | 39771 |

Quench's lane already beats V8's Ignition and JavaScriptCore's LLInt on every fixture. The
JIT-mode gap is the factor that V8's optimizing tiers add over Ignition, 30 to 58 times on
Richards and DeltaBlue. Closing it needs an optimizing online specializer at that level:
shape and callee speculation, inlining, unboxing and allocation elimination. Faster dispatch
or a better baseline cannot close it.

What the diagnostics show:

- **Richards has no native coverage.** `QUENCH_JS_JIT_STATS` reports zero native entries.
  Every hot method is declined: the function tier admits numeric shapes, and OSR admits
  Number webs that exit before every call.
- **The scheduler loop is rejected for a single opcode, `LoadThis`.** Even if admitted, its
  method calls would leave the region on every iteration.
- **The general complete-semantics tier does not help yet.** Enabled with
  `QUENCH_JS_JIT_GENERAL_DISPATCH=1`, it lowers Richards from about 1800 to 1566: it rarely
  compiles within the run, and its bookkeeping costs more than it saves.
- **Pricing is not what blocks it.** Removing the price
  (`QUENCH_JS_JIT_UNPRICED_GENERAL_DISPATCH=1`) gives Richards' small methods native code,
  and two paired runs score 1251 and 1381 against 1651 and 1864 without the general tier.
  The cost is frame entry plus a semantic callout for every uncovered instruction, which
  together exceed what the lane spends.
- **The lane is where Richards' time goes.** Callgrind attributes most instructions to lane
  handlers for script calls (about 15%), field gets and sets (over 30%) and returns.

The conclusion for the design: on object-heavy code, the first projection has to fold what
the online division may know behind guards. That means receiver shapes at field sites,
callees at call sites, and int32 representations. A call has to stay native, without
leaving the region. Folding dispatch alone cannot reach peer scores. The lane already
removes decoding, and a residual of semantic calls is slower than the lane (see the screen
above).

### Frameless lane bodies: measured and set aside

The opt-in lane-body tier (`QUENCH_JS_JIT_LANE_METHODS=1`, `GeneralDispatchMode::Lane`)
runs a declined method's general-dispatch body on its lane activation's registers. It
leaves at exact PCs and resumes accumulator-reading records with their proved register.
It covers:

- receiver reads and receiver field reads and writes;
- capture reads;
- `IncDec`;
- field-cache reads, prototype holders included;
- logged stores.

Paired V8-v7 screens show it slower: Richards -15%, DeltaBlue -8%, RayTrace -12%. A
call-free, fully native 30-record method also runs about 20% slower than the lane (775 to
830 ms against 620 to 675 ms over three runs). The general-dispatch residual keeps every
value tagged in memory and re-proves each guard per instruction, which costs more than the
lane's threaded, record-hinted handlers.

So native calls built on these bodies cannot change the sign. The online specializer needs
a different residual:

- SSA values in machine registers;
- unboxed numbers;
- guards proved once per region and hoisted;
- inline-cached shapes folded as constants.

That is the optimizing tier the critical path below describes.

## JIT redesign plan: an optimizing online specializer

This section replaces the earlier critical path. It comes from a review of the Linear
backlog (TOD-1216 to TOD-1239, TOD-110, TOD-138, TOD-186, TOD-187, TOD-191, TOD-97, TOD-919),
of production engines, and of the measurements above. The target is at least 1.20x the
best of Node and Bun on every V8-v7 fixture in JIT mode, at no more than 0.80x their peak
RSS.

### Why the current tiers cannot reach the target

- **The lane already beats Ignition and LLInt** by 1.2x to 2.8x. The JIT-mode gap is the
  30x to 58x that optimizing tiers add over their interpreters, and removing dispatch alone
  cannot close it.
- **Residuals that keep values tagged in memory lose to the lane.** That includes the
  call-threaded and stencil residuals, the general-dispatch bodies, and frame-entered or
  frameless lane bodies. A call-free 30-record method is about 20% slower. Every guard is
  re-proved on every instruction.
- **Calls dominate object-heavy code.** In DeltaBlue, lane script calls take about 25% of
  time: about 78 instructions and an 88-byte Activation per call. Native code has no
  native-to-native continuation, and OSR regions exit before every call.
- **The representation limits speed:**
  - two inline properties per object;
  - one process-wide prototype epoch that retires every proof at once;
  - Number canonicalization on every float result.
- **RegExp and Splay have separate causes.** RegExp runs on a backtracking interpreter.
  Splay is bound by allocation and collection.

### Target architecture

The division stays the contract above: static facts fold, sites fold behind guards or code
dependencies, and register and heap contents stay dynamic. The optimizer is a *transpiler
of inline-cache state*, as in SpiderMonkey's Warp and CacheIR. Each lane IC record kind
gets one transpile rule into IR guards and operations, written next to the handler that
fills it, so the interpreter's semantics stay the single authority. That is the second
projection applied to feedback.

- **IR.** One speculative SSA graph per unit (a function and its inlined callees, or an OSR
  loop), built in one pass from bytecode and an IC snapshot, as Maglev does:
  - a frame state at every exit;
  - representation selection over tagged, int32, f64 and boolean, with untagged phis;
  - a fixed pass list: check elimination by abstract interpretation, guard hoisting,
    inlining and scalar replacement;
  - lowering through Cranelift;
  - the IR is dropped after compilation.

  `lane_region.rs` becomes its function entry and the OSR webs become its loop passes, so
  the native tiers converge instead of diverging.
- **Exits.**
  - The lane resumes at the exact PC with the window written back.
  - Inlined frames rebuild N lane Activations from a compact varint frame-state table.
  - Scalar-replaced objects are materialized at exit.
  - Per-shape and per-holder code dependencies replace the global epoch for compiled code.
  - Exit counters trigger recompilation with the failing guard made generic.
- **Calls.**
  - A call IC on callee identity.
  - Arguments are written straight into the callee's lane window.
  - A return-site word instead of an eager Activation; the Activation is rebuilt only when
    an exit, a throw or the stack walker needs it.
  - A lane trampoline for callees that are not compiled.
  - Records do not move, so live tagged values are spilled to windows at safepoints and no
    stack maps are needed.
- **Allocation.**
  - Inline allocation from per-space free-record buffers.
  - Inline capacity per construction site, tracking slack as V8 does.
  - Partial escape analysis and scalar replacement across inlined frames.
- **Memory.**
  - Compilation is lazy and priced by payback.
  - A process code budget with eviction.
  - Only exit tables are retained.
  - IC memory is allocated lazily.
  - Cranelift scratch is reused and counted in RSS reports.
  - Copy-and-patch stays the AOT and eager publisher, off the V8-v7 critical path.

### Milestones

Every milestone needs:
- parity across interpreter, `--jit` and forced-JIT modes, and the recovery matrix of
  TOD-191;
- a paired five-round V8-v7 screen, with eleven rounds only at qualification;
- no no-JIT regression;
- peak RSS at or below 0.80x of the JIT peers on all eight fixtures.

Factors are estimates.

| # | Milestone | Main fixtures | Estimated effect | Linear |
| --- | --- | --- | --- | --- |
| M0 | Register-resident region specializer (`lane_region.rs`), exit counters, native/lane attribution. **Gate:** the call-free 30-record method at least 1.5x faster than the lane. | All | Go/no-go for the representation thesis | TOD-1237 |
| M1 | Speculative SSA IR and IC transpiler: own and prototype fields, stores without transitions, this, captures, int32/f64 arithmetic, branches; lane-activation and OSR entry | Richards | about 1.3x; 80% or more of hot records native | new, under TOD-138 |
| M2 | Check elimination, hoisting, per-shape code dependencies, exit-driven recompilation | DeltaBlue, RayTrace | about 1.3x more | new |
| M3 | Native-to-native calls, call IC, window arguments, lazy Activation, lane trampolines, unwinding through native frames | Richards, DeltaBlue, EarleyBoyer | 2x to 3x with M1 and M2 | TOD-1219, TOD-186, TOD-97 |
| M4 | Inlining with per-call-site guards, polymorphic (up to 4) and nested, multi-frame exits | Richards, DeltaBlue, RayTrace | Richards 8x to 15x, DeltaBlue 10x to 20x cumulative | TOD-1219, TOD-1220 |
| M5 | Polymorphic and transitioning property access, inline barrier, element kinds | DeltaBlue, RayTrace, EarleyBoyer | about 1.3x each | TOD-919 |
| M6 | Object representation: per-site in-object capacity, deferred float canonicalization | RayTrace, NavierStokes | about 1.5x | new (heap) |
| M7 | Inline allocation, partial escape analysis, materialization at exit | RayTrace, EarleyBoyer, Splay | 1.3x to 3x | TOD-1221, TOD-1223 (new issue) |
| M8 | Closures: native captures, inline closure creation, code-identity call ICs | EarleyBoyer, NavierStokes | 1.5x to 2x | TOD-1222, TOD-1217 |
| M9 | Int32 kernels: `am3` inlined into loops, checked multiply, bounds-check hoisting | Crypto | 3.3x to 4x | TOD-1218 |
| M10 | Native RegExp tier in the irregexp style, prefilters, cheaper slicing (parallel) | RegExp | 3.5x to 4x | TOD-1224 |
| M11 | Collection throughput: lazy sweeping, nursery rule, Splay all-modes RSS (parallel) | Splay, EarleyBoyer | 2x to 3x with M3, M4 and M7 | TOD-1223 |
| M12 | Code memory discipline: budget eviction, compact exit tables, lazy ICs, scratch accounting (continuous gate) | All | JIT RSS at most 10% above no-JIT | TOD-123 |
| M13 | Qualification: eleven interleaved rounds on Linux and M4, Test262 under forced JIT | All | Acceptance | TOD-1239 |

### What is realistic

- **NavierStokes** already clears the bar.
- **Crypto** is realistic after M1, M4 and M9. **RegExp** is realistic after M10.
- **Splay and EarleyBoyer** are plausible after M3 to M8 and M11.
- **Richards, DeltaBlue and RayTrace** are where TurboFan and FTL are strongest. A
  Maglev-class tier with inlining should reach about 0.4x to 0.8x of the peers. Beating them
  by 20% needs FTL-class polymorphic inlining, allocation sinking and check elimination.
- **Effort.** M0 to M9, a Maglev- or Warp-class tier, is about 9 to 14 engineer-months.
  M10 and M11 take 2 to 3 more, in parallel. All eight fixtures at 1.20x likely take 1.5 to
  3 engineer-years, and the outcome on Richards and DeltaBlue is uncertain.
- **The RSS bar** holds if M12 remains a gate.

Sources:
- [Maglev](https://v8.dev/blog/maglev) and [V8 Lite](https://v8.dev/blog/v8-lite)
- [irregexp tier-up](https://v8.dev/blog/regexp-tier-up)
- [Warp](https://hacks.mozilla.org/2020/11/warp-improved-js-performance-in-firefox-83/) and
  [CacheIR](https://doi.org/10.1145/3617651.3622979)
- [JSC speculation and OSR exit](https://webkit.org/blog/10308/speculation-in-javascriptcore/)
- [LuaJIT allocation sinking](https://ujit.readthedocs.io/en/latest/public/tut-allocation-sinking.html)
- [Truffle partial evaluation](https://doi.org/10.1145/3062341.3062381)
- [PyPy meta-tracing](https://doi.org/10.1145/1565824.1565827)

## Earlier plan: specialization structure

These steps built the specializer structure and the copy-and-patch publication. For V8-v7 they are
superseded by the redesign plan above; the AOT and eager items (TOD-1234, TOD-1236, TOD-1238) stay
off its critical path.

The plan is ordered by dependency, then by expected speed. Each step is a Linear sub-issue of
TOD-1232 and lands with tests in interpreter, `--jit` and native modes.

1. **Specializer structure (P1 and P2 in code)**, TOD-1233. Introduce `vm/specialize.rs`:
   - `BindingTime`, derived from the `opcodes!` field and immediate layouts.
   - The call-threaded static computation `instruction_sites`, which every call-threaded
     publication consumes.
   - The `Specializer` and `GeneratingExtension` traits.

   Route stencils, the Cranelift graph and AOT objects through one call-threaded specializer
   with one generating extension per publication.
2. **Slim stencils**, TOD-1234. Carry the dispatch table as a per-function hole instead of a continuation
   argument. Measure bytes per instruction against Cranelift's graph.
3. **Remove binding-time violations from the semantic callout**, TOD-1235. Pass statically resolved facts
   through the operand record and the entry, not run-time lookups. Measure the call-threaded
   residual against the interpreter on the same workloads.
4. **Fold pure instructions into their stencils**, TOD-1236. Give `Jump`, `Move` and `LoadConst` generating
   extensions that need no callout, written once over the staged semantics that the interpreter
   also uses.
5. **Promote hot call-threaded code to guarded regions**, TOD-1237. The online specializer starts from
   the same instruction sites. Guards cover the site and representation facts it folds.
6. **AOT objects through stencils**, TOD-1238. Emit stencil residuals with linker relocations, so build
   time and the link inputs shrink the same way.
7. **Qualification**, TOD-1239. Run matched JetStream 3 and V8-v7 rounds in both modes on Linux and M4,
   as `jit-design.md` requires. A screen in this file never counts as a win.

## References

- Y. Futamura, [Partial evaluation of computation process](https://www.jstage.jst.go.jp/article/jssst/21/5/21_5_343/_article/-char/en).
- A. P. Ershov, [Mixed computation: potential applications and problems for study](https://doi.org/10.1016/0304-3975(82)90111-6).
- H. Xu and F. Kjolstad, [Copy-and-Patch compilation](https://doi.org/10.1145/3485513).
- H. Xu and F. Kjolstad, [Deegen: a JIT-capable VM generator](https://arxiv.org/abs/2411.11469).
