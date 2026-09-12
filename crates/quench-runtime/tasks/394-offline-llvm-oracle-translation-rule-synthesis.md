# 394 — Offline rustc/LLVM-oracle translation-rule synthesis

Status: planned

Generate high-quality finite stencil rules offline instead of hand-guessing fused handler
shapes. This adapts JavART's offline rule-extraction pattern to the project's stricter
single-semantics and copy-and-patch design. It does not add machine learning, a runtime
compiler backend, or a second selector.

## Pipeline

```text
semantic macro records
  -> bounded typed micro-op forms
  -> rustc/LLVM whole-form compilation matrix
  -> relocation/connector validation
  -> semantic equivalence checks
  -> Pareto frontier {critical path, bytes, patches, spills, seams}
  -> immutable TemplateRecipe catalog
  -> generated pattern automaton and static cost table
```

Enumerate forms from Task 309's semantic grammar and Task 149's finite representation
lattice. Symbolic operands, raw literals, branches, side exits, and physical locations are
holes using Task 390's one patch schema. Enumeration is bounded by named maximum micro-op
count, live-value count, branch count, representation product, catalog bytes, and cooker
time. Alpha-equivalent and commutative forms normalize before compilation.

Pinned rustc/LLVM is an offline code-quality oracle. Compile each legal whole form using
Task 347's validated configurations, extract it with Task 367, and compare it with the
best composition of smaller known rules. Retain only a Pareto winner that shortens a
measured or statically validated dependency chain, removes a seam/conversion/spill, or
uses fewer code bytes without worsening the other named limits.

Every retained rule carries a certificate:

- source semantic form and input/output connector contexts;
- explicit effects, exception/slow exits, ownership behavior, and labels;
- normalized patch manifest;
- differential/property-test generator against generic semantics;
- disassembly and `llvm-mca`/target cost evidence;
- smaller-rule cover it dominates and the exact cost delta.

The runtime receives only immutable recipes, bytes, patch manifests, and a generated
deterministic matcher. V8v7 source, suite identity, paths, literal values, and runtime
execution counts are forbidden training or selection inputs. Use a generated semantic
corpus plus a separately named conformance/training corpus; keep V8v7 as the acceptance
holdout.

## First bounded slice

Enumerate straight-line two-to-six-op I32 forms containing loads, literals, arithmetic,
bitwise operations, comparisons, stores, and one terminal branch. This directly tests
whether Task 385's register-resident region can use a rustc-cooked composite that avoids
the boxed conversion cost which caused Task 381's isolated bitwise leaves to fail.

Compare at least:

- composition of primitive templates;
- the best manually named region template already in the catalog;
- the oracle-selected whole-form template;
- a shared-kernel realization of the same typed morphism.

## Acceptance

- Catalog generation is deterministic and fails closed on undeclared relocations, ABI
  drift, semantic mismatch, or budget overflow.
- Regrouping or alpha-renaming a form yields the same normalized recipe identity.
- Every selected rule passes randomized semantic differential tests including overflow,
  `NaN`, negative zero, side exits, and ownership cleanup as applicable.
- Task 157's selector reproduces brute-force minimum cost on bounded fixtures and can choose
  either a shared kernel or patched instance of the same morphism.
- The first retained production rule removes a verified seam/conversion/critical-path edge
  and passes the complete alternating exact V8v7 gate. Rejected rules remain in the report,
  not in the production catalog.

Primary source for offline translation-rule extraction and lightweight runtime use:
<https://doi.org/10.1145/3720418>. Copy-and-patch's large finite implementation-variant
library supplies the physical target model: <https://arxiv.org/abs/2011.13127>.

## Round-forty-six bounded equality-saturation refinement

Before enumerating physical covers, optionally place each pure, typed, control-free micro-op
island in a bounded e-graph. Apply only equivalences whose JavaScript preconditions are facts
in the island context; effects, allocation, exceptions, observable coercions, NaN/negative-
zero distinctions, and side exits remain explicit boundaries. Extract a small Pareto set by
target latency, bytes, register pressure, and patch count, then send only those forms through
the existing rustc/LLVM oracle and semantic differential checks.

This is an offline catalog-construction tool, never a runtime dependency. Named limits on
e-nodes, iterations, island size, and wall time make saturation fail closed. `egg` supplies
the rebuilding/e-class-analysis algorithm, while Souper supplies evidence that synthesis can
find useful LLVM peepholes; no third-party VM or solver is linked into the engine:
<https://arxiv.org/abs/2004.03082>, <https://arxiv.org/abs/1711.04422>.
