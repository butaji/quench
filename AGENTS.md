# AGENTS.md

This repository implements Node-compatible APIs on JavaScript engine. Treat API shape,
validation, errors, calling conventions, exports, and evidence as data. Generate
repetitive Rust registration and JavaScript wrappers from that data; keep only
irreducible behavior handwritten. Optimize for the minimum maintainable LOC.
Keep the Rust host minimal and do not add or restore a separate runtime crate.
The architecture is defined in `docs/data-first-minimal-runtime.md`.

## Frozen doctrine

1. Never represent the same semantic fact twice.
2. OXC owns syntax; Quench does not invent another syntax tree.
3. Static structure remains data or disappears.
4. VM code represents only dynamic uncertainty.
5. Semantic abstractions do not imply runtime allocations.
6. Share semantic mechanisms; specialize physical execution.
7. One declaration generates every mechanical consequence.
8. Generate mechanics, handwrite observable algorithms, and budget generated
   binary size as well as handwritten source.
9. Facts have three states: `Proven`, `Guarded`, and `Unknown`.
10. Never optimize through observable JavaScript behavior.
11. Keep heap references compact.
12. No subsystem gets its own universe unless semantics truly require it.
13. Types are facts, not another runtime.
14. Profiles are facts, not another optimizer.
15. Optional native execution consumes the same residual Ops, remains bounded
    and disposable, and owns no alternative semantics.
16. If something can disappear before runtime, it must justify why it exists.
17. Complete slow semantics and cheap `Unknown` behavior precede guarded fast
    paths.
18. Generated LOC, binary text, static data, caches, and native code all count
    toward the memory and complexity budget.

## Benchmark integrity — non-negotiable

Quench is a Node-compatible JavaScript runtime. `quench-node` implements only
the Node host/API boundary; it must never identify, rewrite, delegate, or
special-case a benchmark (including V8-v7/Octane fixtures).

`quench-runtime` may keep general-purpose, semantics-preserving optimizations
that are admitted from reusable IR facts and can deopt to the ordinary VM.
It must not contain workload-shaped kernels, benchmark checksum/oracle logic,
fixture-name/source detection, benchmark-only constants, or an alternate
runtime used to obtain a benchmark score. Native execution, when justified,
must consume the same residual Ops and preserve all observable JavaScript
behavior.

Before committing performance work, verify all of the following:

- no benchmark runner, Bun/other-engine delegation, or benchmark build hook;
- no `v8-v7`, fixture-name, suite-marker, expected-score, or checksum gate in
  production runtime/host code;
- every retained optimization is reusable outside the originating workload,
  guarded by facts, and falls back to complete slow semantics;
- Node-compat behavior is tested independently of benchmark score/harness.

If an optimization cannot satisfy those conditions, delete it instead of
keeping a benchmark-shaped exception.

1. Select the next upstream Node fixture or API cluster.
2. Model the reusable API facts in the shared declaration/IR layer first.
3. Generate registration, wrappers, and ordinary tests; hand-write only
   irreducible compatibility behavior.
4. Add a focused stage under `tests/node-compat/stage-N/`, run it, format with
   Prettier, and run `git diff --check`.
5. Commit and push each verified stage before starting the next one.

```bash
cargo build -p quench-runtime
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Run an upstream fixture with:

```sh
tools/run-node-tests.sh tests/node/test/parallel/test-name.js
```

Do not minify or obscure declarations or exceptional polyfills. Generated
mechanical wrappers may replace duplicated source. Leave `tests/node` as the
Node.js submodule and do not modify unrelated external projects.

Do not add or restore GitHub Actions or other GitHub CI configuration. Keep
verification local through the repository tooling.

When behavior is uncertain, first check the actual local Node.js CLI behavior;
then consult the corresponding Node.js source code on GitHub before choosing an
implementation or documenting a compatibility difference.
