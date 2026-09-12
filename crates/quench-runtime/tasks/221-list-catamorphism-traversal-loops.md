# 221 — List-catamorphism recognition for cdr-chasing traversal loops

Status: planned

Concrete companion to [[220-closed-coproduct-tag-dispatch-earley-boyer]] and a direct
application of [[218-initial-algebra-final-coalgebra-data-codata]]'s framing to real
source. `sc_Pair` in earley-boyer *is* the standard `List(A) = 1 + A × List(A)`
functor's initial algebra (a cons cell plus a null terminator), and grepping the actual
corpus finds the shape `while ((l instanceof sc_Pair)) { ...; l = l.cdr }` (or the `l1`/
`sc_l_74`-named equivalent) at twelve distinct source sites in
`/private/tmp/js-engine-benchmark/v8-v7/earley-boyer.js` (lines 3867, 3893, 3946, 4078,
4154, 4168, 4197, 4240, 4268, 4297, 4341, 4420, 4453, 4495 — more than twelve once every
variant is counted). Each one is, by construction, a fold (catamorphism) over that
initial algebra: consume the head, recurse/iterate on the tail, stop at the terminator.

Today each iteration re-derives "is this still a cons cell" via the same `instanceof`
membership test [[220]] targets, then re-loads `.cdr` through the generic property path.
Once [[220]] gives the tag check its O(1) form, this task closes the second half: the
loop *as a whole* should compile to one native pointer-chasing loop — load tag, branch
on terminator-vs-cons, load `.cdr` through a fixed struct-offset (not a shape-guarded IC
re-check every iteration, since the loop body cannot change what `sc_Pair`'s own layout
is), and branch back — with the coproduct-membership check as the single loop-exit
condition rather than a per-iteration re-verified guard plus a separately-scheduled
field load.

This is where [[210-hylomorphism-lowering-fusion]]'s general recipe meets a real,
frequently-occurring instance: recognizing this exact loop shape (guard-typed backward
edge over a self-referential single-field coproduct member) as a catamorphism gives it a
correctness argument for free (termination follows from the initial algebra being
well-founded — no cyclic `sc_Pair` chain can exist from a well-formed program, which the
implementation should assert rather than assume) instead of treating it as one more
opcode pattern needing its own hand-verified stencil.

Concrete steps:
1. Recognize the loop shape in the compiler: a `while`/`for` backedge whose condition is
   a closed-coproduct tag test (per [[220]]) on a loop-local binding, and whose body's
   last write to that binding is a load of one fixed field from the same binding's
   current value (the "cdr" position).
2. Compile the recognized shape to one native loop: tag-load, branch-on-terminator,
   field-load-through-fixed-offset, backedge — reusing [[123]]/[[124]]'s traced
   numeric/dense loop stencil machinery as the template for a traced *list* loop family
   instead of a numeric one.
3. Verify termination is assumed only under the initial-algebra well-foundedness
   argument, not silently — add a cycle-safety note or bounded-iteration diagnostic
   consistent with [[75-instruction-budget-diagnostics]] for the case a malformed
   (cyclic) structure reaches this path despite the source-level guarantee.

Acceptance: at least the twelve identified `while (l instanceof sc_Pair) {...; l = l.cdr}`
call sites compile to the fused native list-traversal loop with no per-iteration IC
guard re-check on the fixed `cdr` field access, verified by instruction count against
the unfused baseline; a rerun of [[132]]'s residual profiler shows the associated
name/environment and static-property residual entries attributable to these loops drop;
earley-boyer's score improves in an alternating A/B with every suite at or above the
standing floor; a defensive cycle/budget check exists and does not fire on any correct
program in the test corpus.

Source: `/private/tmp/js-engine-benchmark/v8-v7/earley-boyer.js` (local V8v7 corpus
checkout, lines cited above).
