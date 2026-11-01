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

### Critical path for V8-v7

The plan steps keep their order for AOT and startup. For V8-v7 score, the critical path is
step 5, widened:

1. **An online method specializer** whose division folds Site contents behind guards.
   - Guarded own and prototype field reads and writes.
   - Monomorphic method calls that enter the callee's native code directly.
   - Branches on tagged values.
   - Every other instruction residualized through its semantic entry, so coverage is total.
2. **A native-to-native call continuation** that keeps the activation frameless. A call no
   longer exits to the VM; this is the rooted VM continuation that `jit-design.md` already
   lists.
3. **Inlining of small monomorphic callees** inside the specializer, with exact-PC frames for
   recovery.

This is the method-JIT foundation tracked by TOD-1219 (Richards), and DeltaBlue, RayTrace
and EarleyBoyer depend on it too. Crypto and NavierStokes depend on the numeric OSR tier,
which `v2-jit` is extending.

## Plan

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
