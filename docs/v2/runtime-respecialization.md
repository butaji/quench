# Function-level runtime re-specialization

Function-level bytecode replacement is mechanically possible, but it does not
make observed values into sound static facts. This note separates the swap
mechanism from the semantic transformation it would need to pay for.

## Current ownership boundary

rqj deliberately stages compilation and execution into different processes:

1. the compiler parses source with OXC and produces a `ResidualProgram`;
2. the CLI serializes that residual and releases the source/parser pages;
3. a clean child process reads only the residual and calls `Vm::execute`;
4. the VM borrows the residual immutably for the complete execution.

The execution process therefore has neither source nor an OXC AST to
re-specialize. Retaining them would undo the clean-residual RSS boundary. A
bytecode-to-bytecode optimizer could run in the child, but that is a different
generating extension and still needs a sound transformation that wins the
cumulative benchmark gate.

## Safe replacement protocol

A future mutable residual store would need versioned function bodies. Each
frame would pin `(function, version)` at call entry; promotion would publish a
new version only for later calls, and the old body, handler table, register
root map, field/method-site metadata, and superinstruction data would remain
alive until the last pinned frame returns. Replacing `functions[id].code` in
place is incorrect because a recursive or currently executing frame's `pc`
belongs to the old body.

Publication would happen only at the call boundary. Unpromoted functions
would pay no dispatch branch: their closure target would already identify the
current body version. The number of retained versions would be bounded, and a
failed or invalidated candidate would publish the generic version rather than
patching active frames.

## Soundness boundary

Call count and frequency may decide *when* to optimize. They do not prove a
value, branch, type, or receiver shape invariant for later calls. An observed
fact can influence only a semantics-preserving layout unless the residual
keeps a guard and generic fallback. In particular:

- deleting an unobserved branch is unsound;
- substituting an observed value or shape is unsound;
- direct field access must retain the ordinary shape check and miss path;
- numeric specialization must retain a type check and generic coercion path.

The latter two are inline-cache/adaptive-instruction semantics. Moving their
installation trigger to a function call count amortizes installation, but it
does not remove the steady-state guards.

## Feasibility result

There is currently no additional function-level semantic transformation to
install:

- task 14 implemented the guarded observed-type form. It was Score-neutral
  and added 65,536 bytes of median RSS, so it was removed;
- task 83 proves that removing those guards would make correctness depend on
  the training run;
- tasks 164--166 implement the strongest current guard-free layout candidate
  exposed by profiling, including an unrolled numeric fast-path residual.
  Its best local Crypto result regresses Richards and RSS, so it was removed;
- the retained inline caches already install shape-specialized entries lazily
  while preserving checks and miss paths.

Adding source retention, mutable/versioned residual ownership, promotion
counters, and body retirement before a residual transformation passes would
measure infrastructure overhead alone. It cannot demonstrate payback and
would violate the repository rule against retaining unused speculative
machinery.

## Decision

Do not integrate runtime body replacement now. The safe versioning protocol
above is the required design if a future, source-agnostic bytecode
transformation first passes the offline cumulative gates. Only then is it
meaningful to spike whether in-process installation pays back its counters,
extra live body, and publication cost. Observed facts will never be installed
as unguarded semantics.
