# Repository rules

Quench implements JavaScript semantics and Node-compatible APIs.

- Use Rust for implementation and Rust macros for code generation. JavaScript is guest/fixture input; existing measurement tools remain usable.
- Keep Node host/API behavior in `quench-node` and JavaScript semantics in `quench-runtime`. OXC owns JavaScript syntax.
- Share semantics across interpreter, specialized, and native execution. Optimizations require proven assumptions, dependencies whose change invalidates the code that relied on them, or a guard proved once with exact fallback (see Execution laws).
- Verify changed Node behavior against the local Node oracle and relevant upstream source: values, descriptors, identity, ordering, errors, exit status and host effects.
- Production behavior must never depend on benchmark identity, fixture source, scores, checksums, suite markers or the comparison engine.
- Qualify performance on the available execution host; record its exact platform and resource limits, and preserve existing platform behavior.
- Resolve static structure before execution. Justify allocations, metadata, caches and generated code by semantic necessity or measured benefit.

## Execution laws

Futamura's projections and Ershov's mixed computation are the design: every native tier
is the interpreter specialized to what is known, and each fact has a binding time
([specialization architecture](https://linear.app/todoxapp/document/quench-repository-docsv2specializationmd-7c38f4f2f78e)).

1. Check once. A dynamic fact is proved at most once per lifetime of the value or region
   that carries it; every later use reads the proof (a type, a representation, a folded
   constant). Re-checking a fact the model already holds is a bug, not a safety margin.
2. Reactive, not guarded. A fact that rarely changes (a shape's layout, a prototype chain,
   a constant field, a callee's identity) is a dependency: the change triggers
   invalidation of what relied on it, and the users pay nothing per use. Guards remain
   only for facts that vary per execution, and are hoisted to where they are proved once.
3. O(1) per operation. Hot paths do constant work: no scans, walks or searches per use;
   anything proportional moves to the event that changes it.
4. C-level residuals. Generated code is judged against the C a programmer would write for
   the same specialized program: values in registers, numbers unboxed, layouts as
   constants, no boxing, spills or checks C would not need.

## Redundancy laws

Nothing is stored, checked or computed twice, at any level.

1. One fact, one home: a value, flag, table or constant is defined once and derived
   everywhere else. `quench-lint` rejects token-identical functions and repeated
   module-level constants; clippy's redundancy lints (workspace `Cargo.toml`) reject
   second lookups, copies and eagerly built values.
2. Layers trust each other: the parser, bytecode compiler, interpreter, JIT tiers, OSR and
   AOT share one representation of each proven fact. A later layer reads what an earlier
   one established (a resolved binding, a shape, a type, a bound) and never re-derives or
   re-verifies it; there is no adapter glue converting one layer's view into another's.
3. Interfaces are built for efficiency: a boundary passes the producer's representation
   (indices, borrowed slices, packed words), not copies, re-encodings, boxed values or
   per-call conversions; a protocol that needs translating on every crossing is redesigned.

## Lisp mindset manifesto

Every line carries domain meaning. Code is a liability; data is leverage.

1. Model data and invariants before behavior: the domain, its rules and its config are
   plain data before any function. When code feels awkward, the model is wrong.
2. Prefer pattern dispatch over duplicated decision trees.
3. Unify concepts with shared semantics: seek the one representation in which separate
   cases become the same case.
4. One fact, one authoritative representation.
5. Derive metadata, wrappers and execution views from that authority.
6. Derive state from explicit transitions; memoize only where useful.
7. Model lifecycle behavior as named state machines.
8. Keep effects explicit and at the boundaries: a function either computes or acts.
9. Deduplicate knowledge, not merely similar syntax.
10. Prefer shallow, named composition over frameworks: small operators in the domain's
    vocabulary with uniform inputs, outputs and failures, keeping the laws callers rely on.
11. Zero magic numbers: every representation bit, spec boundary, protocol
    value, sentinel, threshold, and non-obvious numeric rule has a named owner
    or is derived from a named layout. Keep literal zero/one where they are
    ordinary indices or arithmetic identities, not hidden semantic policy.

## Data-oriented design

Process contiguous data in batches, not scattered objects.

- Replace hash tables with flat arrays. Store indices (not pointers) and pack frequent
  flags into the high bits of the index.
- Caching is a symptom of slow design. Remove caches and redesign the core path so the
  work is cheap enough that caching is unnecessary.
- Prefer structure-of-arrays over array-of-structures so hot fields stay contiguous and
  vectorizable.
- Prefer stack allocation with a small inline capacity over heap-allocated lists.
- Use a bitset instead of an array of bools.

Functions ≤ 40 lines with cyclomatic complexity ≤ 10, files ≤ 500 lines: split along a
domain seam, never exempt. `quench-lint` enforces it; code that predates the law is listed
in `crates/quench-lint/oversized.txt`, which may only shrink (`quench-lint --update` after
a split). There is no architectural ceiling on region size, register allocation, SIMD or
additional optimization tiers. Compilation/cache
budgets need a documented cost or correctness purpose and safe exhaustion behavior.
Do not force recursion, event sourcing or abstractions into ordinary Rust.

Apply this discipline repository-wide to production code, tools, and task
automation. Before finishing a change, do a reduction pass: identify the single
authoritative representation for each fact, route equivalent behavior through
the shared domain operation, derive projections instead of storing copies, and
remove redundant branches or one-use wrappers that add no domain meaning. Also remove
defensive checks for states the model excludes, wrappers that only rename, comments
that restate code, unused parameters and flags, and configurability nobody asked for.
Stop when removing anything more would lose meaning.
Keep similar-looking rules separate when their semantics differ. Treat these as
review requirements, not optional style preferences.

## Work and evidence

[The Linear project](https://linear.app/todoxapp/project/quench-abb744c79c4e/overview) owns backlog, status, lane ownership and
coordination; its overview holds the lanes and sync rules. Documentation, contracts,
commands, plans, handoffs, historical measurements and progress artifacts live only
in Linear. Do not create or restore local documentation; reconcile documentation
brought in by merges with its existing Linear home before removing local copies.
The [Linear document index](https://linear.app/todoxapp/document/quench-repository-document-index-11102c539c53)
locates contracts, commands and design references.
The repository holds code, executable tools, tests, fixtures and build inputs;
do not add progress or evidence directories here.
Online research is a required step before designing or implementing a task: survey
the relevant literature (arXiv papers included), upstream engines and specifications
to understand the scope of the work and the known solutions and their trade-offs.
Record the sources that shaped the chosen approach in the task's Linear issue.
Measure V8-v7 at meaningful milestones after the affected execution path passes
its safety/correctness checks. Evidence is a completion check on each critical-path
state transition, not a separate prerequisite or replacement for the backlog order;
focused measurements may run earlier when they illuminate an affected path. A
functional milestone can complete without a speed claim; performance claims require
matched production measurements and regression analysis.
