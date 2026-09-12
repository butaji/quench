# 377 — Weighted residual-frontier hypergraph planner

Status: in_progress

Turn residual block profiles into one canonical data set that exposes capability
complementarity. For every normalized block, record:

```text
ResidualBlock = {
  missing_families,
  dynamic_entries,
  bytecode_count,
  native_prefixes,
  helper_edges,
  current_tile_cost
}
```

A candidate family set closes a block exactly when it covers every member of that block's
`missing_families`. Aggregate identical missing sets and mine frequent weighted sets so
`LoadName + Call` is visible as one high-value frontier instead of two unrelated opcode
counts. Search a named bounded capability depth with greedy marginal gain plus bounded
beam/local improvement. Score complete candidates by dynamic entries and the measured
physical costs from Task 157: copied bytes, unique kernel bytes, patches, guard branches,
frame traffic, and native/kernel/native transitions.

This planner is diagnostic data reduction, not a runtime optimizer. It emits ranked JSON
as the canonical artifact and derives Markdown tables from it. Benchmark and source names
may label evidence reports but never enter lowering, stencil selection, or generated code.

Use named search-depth, beam-width, catalog-budget, capability-cost, and helper-edge
constants. Do not encode an unnamed minimum event count or hard-code the currently
observed opcode combination.

Categorically, each capability is a family of morphisms and each block is a demand for a
joint cover. The planner chooses generators; the existing free-monoid/category kernel
still performs composition. In Lisp terms the residual profile is quoted data, ranking is
a pure rewrite/reduction, and reports are derived views.

Acceptance: the tool reproduces Task 373's singleton and combination closure counts,
ranks Task 375's equality family as a real positive-reach candidate, exposes name+call and
computed+numeric combinations, is deterministic under input reordering, and has tests for
zero-cost, over-budget, tied, dominated, and complementary candidates. A selected
optimization still needs the normal exact A/B gate; planner score is never acceptance.

Primary algorithm sources: weighted transaction/itemset discovery
<https://rsrikant.com/papers/vldb94.pdf>, budgeted maximum coverage
<https://doi.org/10.1016/S0020-0190(99)00031-9>, and costed compiler graph covering
<https://llvm.org/pubs/2008-06-LCTES-ISelUsingSSAGraphs.pdf>.

## Implemented slice and evidence

`scripts/residual-frontier.py` now parses the nine Task 373 residual profiles into one
canonical block table, classifies missing capability families, records first-missing
family/native runs/helper edges, enumerates bounded joint covers, prunes dominated
candidates, and emits canonical JSON plus a derived Markdown view. Runtime lowering does
not consume the report. `scripts/test_residual_frontier.py` has six deterministic tests
covering complementary closure, zero cost, over-budget exclusion, ties, dominance, and
input-order independence.

Generated evidence is in `reports/task377-residual-frontier/task373-frontier.json` and its
derived `.md` view. It contains 759 distinct residual block shapes and 20,570,009 dynamic
entries: 17,944,697 structurally unsupported entries and 2,625,312 current guard misses.
It exactly reproduces the manually checked closure counts:

- strict equality: 662,575; loose equality: 366,592; both: 1,029,167;
- computed: 460,479; name: 438,003; name plus strict equality: 1,597,120;
- bitwise plus computed plus numeric unary: 977,987;
- call alone: 4,060,419; call plus name: 5,502,818.

The new result changes priority wording: call is the largest structural frontier, but the
already rejected helper-calling call trials show that reach is not implementation cost.
Task 379's hostless, fully surrounded native call continuum remains the strategic target;
Task 378 is a smaller directly buildable closure family and useful surrounding coverage.

The task remains in progress because candidate costs are still unit weights and each
block's `current_tile_cost` is intentionally `null`. Completion requires importing Task
157's measured copied bytes, shared kernel bytes, relocations, guards, frame traffic, and
native/kernel transition costs, then verifying that rankings change deterministically
under a non-unit cost fixture.

## First measured physical sample

Task 378 supplies the first accepted post-planner sample. Its lazy direct `LoadName` IC
adds 52,528 copied code bytes, closes 129 additional direct blocks and 586 direct opcodes,
and increases native entries by 2,005,271 in the first exact process record. The exact
aggregate improves 1.45% with a 95% interval of `[+0.67%, +2.13%]`.

The three rejected snapshot variants are equally important cost evidence: static closure
reach did not price eager per-activation refresh, and those variants lost 5.84%, 5.22%,
and 0.23%. The physical model therefore needs distinct terms for copied bytes, per-frame
setup, per-use guards, slow ownership arms, and closed native entries. A family name alone
cannot be assigned one scalar cost independent of its realization.

## Post-Task378 refresh and realization feedback

The planner now accepts repeatable `--supported-atom` arguments and records the sorted
effective atom set in its canonical JSON. Atom granularity matters: Task 378 implements
`LoadName`, but not `StoreName` or `DeclareName`, so marking the whole `name` family as
supported would invent coverage. Its input edge now reduces both raw
`JIT_STATS_JSON:` output and the standard benchmark routine's nested JSONL records. Nine
tests cover these additions and the prior search laws.

`reports/task381-post-name-residual/frontier.json` is a fresh eight-suite, 20 ms physical
sample from the accepted Task 378 binary. It records 5,912,503 residual block entries,
4,852,796 structurally unsupported entries, and 1,059,707 guard misses. Under this sample,
computed alone closes 140,375 entries; computed plus bitwise closes 351,535; adding numeric
unary reaches 352,292.

Task 381 validates and corrects that static ranking. Direct dense computed access passed
its exact gate at +1.57% `[+0.03%, +3.36%]`. Adding six bitwise and three numeric-unary
templates then passed a short screen at +1.14%, but failed the prescribed nine-pair gate:
aggregate -3.45% `[-8.65%, +0.49%]`, with Crypto, RayTrace, and Earley-Boyer below the
suite floor. Those leaves were reverted. The physical cost model must therefore charge
conversion-heavy template size and code-cache footprint rather than equating an extra
closed block with an equal benefit.

The accepted computed realization supplies a second physical sample: +165 linked direct
blocks, +1,922 direct opcodes, and +105,876 copied function-code bytes in the first exact
record. These values belong to the realization, not the abstract `computed` family, and
will feed the pending non-unit cost fixture.
