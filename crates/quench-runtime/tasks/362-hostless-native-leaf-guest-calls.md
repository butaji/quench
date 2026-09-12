# 362 — Hostless native leaf guest calls

Status: complete

Implement the first executable Task 146/181 guest-call continuum for statically pure,
noncapturing, no-`arguments`, straight-line user callees. The successful call edge must:

1. guard the monomorphic callee identity, published immutable recipe, stable reusable
   activation, exact arity, and non-reference-counted receiver/argument/destination words;
2. initialize the already-cleared child frame directly from the caller frame;
3. install an explicit caller frame, caller site, and native return continuation;
4. tail-transfer to the callee's post-prologue `guest_entry` without a Rust ABI call;
5. transfer the result, clear child value slots, release the activation-busy flag, restore
   the caller connector, and branch directly to the caller continuation from one shared
   immutable return kernel.

The callee-safety proof initially permits only local/immediate loads, local stores, moves,
non-string-producing unary operations, arithmetic/comparison operations, and one terminal
return. It rejects calls, allocation, names, properties, strings, regexps, control flow,
handlers, and exceptions. These are explicit missing context/effect obligations, not
runtime hotness decisions or benchmark-shaped exclusions.

`FunctionCallRecipe` remains the canonical immutable fact. `InlineCallTarget` carries only
per-site mutable publication and continuation fields. The child activation remains owned
by `CallIcSite`; native execution borrows its stable address under a single-threaded busy
flag so recursive re-entry takes the canonical slow path rather than aliasing it.

Categorically, the call is a partial morphism selected by the product context
`Identity × Arity × Ownership × Effects × Activation`. The shared return kernel and copied
call stencil obey the same connector category; physical sharing does not change their
composability. The Lisp staging order remains `quote -> derive leaf safety -> link
continuations -> execute`.

Preflight: Task 343 measured exact straight-line leaf reach of 15.81% of Richards calls,
37.88% of DeltaBlue, 14.02% of RayTrace, 10.54% of Splay, and 95.33% of RegExp. Tasks 355
and 357 prove that another Rust helper is invalid and that post-prologue entry plus an
explicit return target are already available.

Acceptance requires structural/layout tests, executable nested entry/return tests,
disassembly with no call instruction on the successful call/return path, all release and
GC-stress tests, native hit/miss counters, and a complete alternating V8v7 A/B. Retain only
if it clears the standing component and aggregate floors.

## Result: executable prototype rejected and removed

The experiment implemented the complete proposed physical edge: a rustc/LLVM-cooked
monomorphic call stencil, post-prologue guest entry, explicit caller/site/continuation
words, a stable reusable child activation, and one shared immutable return kernel. The
successful call and return edges used tail transfers only. A 20-byte cooked probe also
memoized the static leaf-safety decision at IC publication so ineligible callees could
skip the roughly 900-byte full guard/setup fragment.

Runtime rejection counters falsified the reach assumption. Representative 20 ms runs
recorded:

- Richards: 181,090 attempts, zero hits; 175,227 failed the semantic leaf proof.
- DeltaBlue: 495,967 attempts, 1,210 hits; 459,640 failed the semantic leaf proof.
- RayTrace: 225,477 attempts, zero hits; 147,080 sites were unpublished and 66,859 failed
  the semantic proof.
- RegExp: 223,786 attempts, zero hits; 207,288 sites were unpublished and 16,496 failed
  the semantic proof.
- Splay: 423,088 attempts, zero hits; 405,515 failed the semantic proof.

Ownership, activation, snapshot, and frame-layout guards contributed essentially none of
the misses. The dominant fact is categorical: the narrow call morphism's input object is
almost never produced by the current bytecode lowering. The earlier static candidate
census measured a different abstraction and therefore overstated executable reach.

Widening the safety predicate to pure control flow produced native hits but was unsound:
the full RayTrace smoke later reached `multiplyScalar` with an undefined argument. That
widening was immediately reverted. The correct narrow prototype passed the complete
V8v7 correctness smoke under `DEEGEN_DIRECT_CALL_REGIONS=1`, but scored 2184.65 at 20 ms.
The accepted Task 361 binary scored 2236.70 in the paired short run. Splitting every call
also destroys batching in adjacent generic block kernels: an ineligible call pays the
probe and then a separate call-only slow-kernel entry before the suffix resumes.

A three-pair 200 ms check of the dormant implementation measured median 2411.78 baseline
versus 2388.57 candidate (-0.96%). All Task 362 runtime/AOT fields and handlers were
therefore removed, not left as default-path memory overhead. The restored source passes
all 132 release tests. A final 100 ms complete-suite smoke scores 2408.43 with binary SHA
`a1bcd95baf8b922ee59fd74cfcd7cc71ddd87fb3e10deac185c33daba4b0e6d1`.

This is not evidence against guest continuations. It is evidence that they cannot be
introduced as an isolated fine-grained tile while most neighboring semantics remain
coarse generic kernels. Revisit only after Tasks 309/157/316 provide total native cover,
or as a whole call-containing region whose entry/exit contexts include all ownership and
control effects. That is the multi-level monoid requirement: opcode, block, loop, and
function morphisms must compete in one cover rather than forcing every bytecode into the
smallest granularity.
