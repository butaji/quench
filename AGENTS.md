# Repository rules

Quench implements JavaScript semantics and Node-compatible APIs.

- Use Rust for implementation and Rust macros for code generation. JavaScript is guest/fixture input; existing measurement tools remain usable.
- Keep Node host/API behavior in `quench-node` and JavaScript semantics in `quench-runtime`. OXC owns JavaScript syntax.
- Share semantics across interpreter, specialized, and native execution. Optimizations require proven assumptions or runtime guards with exact fallback.
- Verify changed Node behavior against the local Node oracle and relevant upstream source: values, descriptors, identity, ordering, errors, exit status and host effects.
- Production behavior must never depend on benchmark identity, fixture source, scores, checksums, suite markers or the comparison engine.
- Qualify performance on the available execution host; record its exact platform and resource limits, and preserve existing platform behavior.
- Resolve static structure before execution. Justify allocations, metadata, caches and generated code by semantic necessity or measured benefit.

## Lisp mindset manifesto

1. Model data and invariants before behavior.
2. Prefer pattern dispatch over duplicated decision trees.
3. Unify concepts with shared semantics.
4. One fact, one authoritative representation.
5. Derive metadata, wrappers and execution views from that authority.
6. Derive state from explicit transitions; memoize only where useful.
7. Model lifecycle behavior as named state machines.
8. Keep effects explicit and at the boundaries.
9. Deduplicate knowledge, not merely similar syntax.
10. Prefer shallow, named composition over frameworks.
11. Zero magic numbers: every representation bit, spec boundary, protocol
    value, sentinel, threshold, and non-obvious numeric rule has a named owner
    or is derived from a named layout. Keep literal zero/one where they are
    ordinary indices or arithmetic identities, not hidden semantic policy.

Functions and modules should be cohesive and reviewable. There are no mandatory
line-count or complexity caps. There is no architectural ceiling on region size,
register allocation, SIMD or additional optimization tiers. Compilation/cache
budgets need a documented cost or correctness purpose and safe exhaustion behavior.
Do not force recursion, event sourcing or abstractions into ordinary Rust.

Apply this discipline repository-wide to production code, tools, and task
automation. Before finishing a change, do a reduction pass: identify the single
authoritative representation for each fact, route equivalent behavior through
the shared domain operation, derive projections instead of storing copies, and
remove redundant branches or one-use wrappers that add no domain meaning.
Keep similar-looking rules separate when their semantics differ. Treat these as
review requirements, not optional style preferences.

## Work and evidence

[The Linear project](https://linear.app/todoxapp/project/quench-abb744c79c4e/overview) owns backlog, status, lane ownership and
coordination; its overview holds the lanes and sync rules. `evidence/` holds
measurement artifacts. [Documentation](docs/README.md) owns contracts and
commands.
Measure V8-v7 at meaningful milestones after the affected execution path passes
its safety/correctness checks. Evidence is a completion check on each critical-path
state transition, not a separate prerequisite or replacement for the backlog order;
focused measurements may run earlier when they illuminate an affected path. A
functional milestone can complete without a speed claim; performance claims require
matched production measurements and regression analysis.
