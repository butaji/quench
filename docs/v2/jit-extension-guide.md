# Extending the shared JIT foundation

This is the consumer contract for the implementation, alongside
[jit-design.md](jit-design.md). These are internal Rust extension seams, not a
versioned external ABI. Keep JS and Wasm on the shared entry, executable-memory,
interrupt and budget machinery. Extend a semantic lowerer rather than adding a
package recognizer or a second runtime. Downstream lanes own workload tuning and
score/RSS qualification; a foundation feature needs semantic and native-body proof.

## Acceptance matrix

| Mechanism | Existing seam | Consumer acceptance | Remaining limitation |
| --- | --- | --- | --- |
| Semantic lowering | `vm/compiled.rs` shared numeric emitters; `vm/compiled/osr_loop.rs` analysis/lowering | Boundary values, overflow, negative zero and interpreter parity | Function and OSR semantic lowerers are not fully unified |
| Publication and entry | `vm/jit_backend.rs`, `CodeMemory`, `Entry`, `enter_native` | W^X publication, budget exhaustion fallback, interruption | Internal ABI changes require all caller/host paths to be reviewed |
| Hot-loop OSR | `LoopRegion`, `LoopEntry`, `LoopSite`, `lane_loop_native` | Real completed native work, exact header/backedge identity | Admission is deliberately restricted; native entry alone can be tail-only |
| Fields and arrays | Existing shape/layout guards and lifetime-based receiver analysis | Receiver replacement, aliases, holes/prototypes, bounds, growth and GC | A new shape must extend the existing guard contract, not bypass it |
| Calls and inlining | `analyze_inlining_osr_region`, `InlineFrame`, `InlineGuard`, `lane_inlined_target` | Identity/reassignment, strict/sloppy receiver binding, arity, reentrancy | Captured leaf subset only; general effectful native call continuation remains incomplete |
| Recovery | Region origin PCs, `LoopOutcome`, `lane_inlined_activation` | Exact activation/PC/live values; no repeated effect; thrown identity | Recovery mapping must be extended with every newly admitted effect |
| Roots and allocation | VM register root masks, existing guarded callouts, GC-debt exit | Live values survive GC; backing pointers reacquired after growth | There is no permission to call arbitrary allocating guest helpers from native code |
| Policy and tuning | `MemoryPolicy`, shared tier/code-cache accounting, named OSR limits | Exhaustion falls back safely; policy-specific behavior measured separately | No stable runtime knob for every heuristic; edit the named owner with tests |
| Diagnostics | `QUENCH_JS_JIT_STATS`, per-site OSR records, opt-in trace and dump | Separate analysis, eligibility, compile attempt, native run and exit evidence | Some compile failures are generic; inline eligibility does not expose a structured reason for every rejected precondition |
| Consumer checks | Runtime regressions below and guest example | Node/interpreter/JIT parity plus separate native-body assertions | Example success does not qualify a package or establish consumer adoption |
| Delivery | Exact commit, binaries, corpus/policy hashes and local reports | Reproducible build and consumer verification on the delivered revision | Shared publication and remote adoption require their own authorization and evidence |

All Rust paths above are relative to `crates/quench-runtime/src`.

## Add one supported operation

1. State the value/effect contract first: input representations, outputs, throws,
   calls, allocation, alias writes and interrupt behavior. Reuse shared semantics.
2. Extend analysis and lowering together. An unsupported shape must remain an
   explicit decline or an exact pre-effect exit. Preserve original operands for
   floating fallback and the authoritative source PC for recovery.
3. For a call, preserve program/function identity, parameter window, receiver
   binding, activation room and exception state. Field-derived callees and
   captured callees have different identity guards. Do not erase this distinction.
4. For allocation or reentry, publish live roots and resumable frame state before
   the helper. Reacquire storage afterward. A successful effect must not be
   overwritten by stale SSA writeback or replayed on exit.
5. Add an adversarial runtime test and an independent guest parity case. Assert
   completed native work beyond call-boundary exits. Test guard failure after
   successful warmup, not just initially unsupported inputs.
6. Run affected tests on a fixed source and separate build target; preserve the
   control binary. Use actual workloads to check the mechanism, not to special-case
   production behavior. Keep instrumented and uninstrumented results separate.

## Captured leaf example

`examples/jit/captured-leaf.cjs` exercises a depth-zero captured identity function,
then replaces it. Run from the repository root with the binaries you built:

```sh
node examples/jit/captured-leaf.cjs
target/production/quench-node --no-jit examples/jit/captured-leaf.cjs
target/production/quench-node --jit examples/jit/captured-leaf.cjs
QUENCH_JS_JIT_STATS=1 target/production/quench-node --jit examples/jit/captured-leaf.cjs
```

All modes must print `[160000,18000]`. In the separate stats run, find `capturedLeafLoop`
by source and function name, then its OSR site. Require native runs greater than
exits plus 20 and correct fallback output after reassignment. Capture identity
rejection occurs in the VM before native entry; it need not increment the native
`entry_guard_misses` counter. Function-tier
`native_entries` is a different counter and may be zero for a working OSR loop.
The guest is a mechanism example, not a benchmark or a timing assertion.

The current inliner requires exact argument count, stable setup, and an eligible
receiver-independent leaf body. It does not thereby support a callee's own capture
reads, arbitrary nested calls or global resolution. For example, Zod's locale
`getTypeName` uses its own dictionary capture and calls `Number.isFinite`/`String`;
its expected-type call also omits one argument. A captured-identity success does not
establish support for that helper. Acyclic field-call functions such as the reported
Zod `operations()` are a separate function-entry problem with no loop OSR site.

Focused tests in `vm/tests.rs`:

- `shared_osr_inlines_plain_captured_callee_with_identity_guard`
- `shared_osr_inlined_callee_leaves_through_its_activation`
- `shared_osr_captured_and_parameter_callees_preserve_gc_throw_and_coercion_once`
- `shared_osr_exact_effect_exit_preserves_prefix_across_gc_and_throw`
- `shared_osr_reloads_changing_field_receivers_and_exits_before_effects`
- `shared_osr_append_growth_refreshes_aliases_and_recovers_gc_debt`
- `shared_osr_nested_array_reads_reload_receivers_and_preserve_effects`

## Tune policy without changing semantics

Production qualification uses plain `--jit` with diagnostic and experiment flags
unset. `MemoryPolicy` selects the named function-tier and loop optimization policy
in `compiled.rs`; OSR hotness, decline, exit and compilation limits have named owners
in `dispatch_fast.rs`. Shared code accounting belongs to `program_store.rs`.
These are policy interfaces for source changes, not an invitation to expose new
per-package environment switches or raise budgets to mask repeated exits.

`QUENCH_JS_JIT_GENERAL_DISPATCH=1` is an opt-in experiment, not default evidence.
Wasm policy flags are revision-specific: do not reuse an old label as proof of the
same admission behavior. Record source, flags and coverage for each experiment.
Changing a default requires shared-owner coordination, fixed inventories and
matched correctness/performance evidence. Exhausted budgets must still interpret.

## Read diagnostics honestly

- `compile_attempts == 0` is not a code-generation rejection. Check threshold,
  policy, entry observations and eligibility first.
- Per-site `failure_pc`/`failure_reason` describe analysis. A generic function-tier
  unsupported result does not identify a missing opcode or general-dispatch cause.
- `deferred_call` and `call_boundary_pcs` describe the selected region. A region
  can enter natively and exit before every useful operation.
- Compare `native_region_runs`, `exits`, guard misses and widenings with a known
  semantic checksum. Counters do not measure CPU share or speed.
- `QUENCH_JS_JIT_TRACE_OSR_REJECT` supplies additional entry/exit attribution;
  `QUENCH_JS_JIT_DUMP` writes native/IR artifacts. Keep their cost out of timings.
- Missing attribution remains unknown. Do not invent a guard reason from a null
  field. A future typed inline-rejection result should be emitted at the existing
  authoritative eligibility check, rather than repeating admission logic in a
  diagnostic-only analyzer.

## Reproducible validation and handoff

Use the normal locked production build in a separate target directory. Record
source cleanliness, toolchain, build flags, binary hashes and corpus revisions.
Keep raw reports outside Git. Full Test262 uses the canonical `run-all`, unchanged
Node waivers and 30000 ms case deadlines; full Wasm uses `quench-wasm-test`'s `run`
and 60000 ms file deadlines. Run both modes. Preserve timeouts, crashes, raw
failures and waiver adjudication. Historical full-suite green is source-specific.

The local delivery packet must list completed tests, remaining unsupported shapes,
policy defaults and open acceptance gates. Dynamic instruction-count A/A cannot be
replaced by static assembly counts or wall time. Downstream score/RSS goals remain
open until their owners qualify them; neither example success nor conformance
marks those goals complete. Publication and verified consumer adoption are separate
from a locally usable implementation.
