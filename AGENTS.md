# quench-node development guide

This repository implements Node-compatible APIs on rquickjs. Treat API shape,
validation, errors, calling conventions, exports, and evidence as data. Generate
repetitive Rust registration and JavaScript wrappers from that data; keep only
irreducible behavior handwritten. Optimize for the minimum maintainable LOC.
Keep the Rust host minimal and do not add or restore a separate runtime crate.
The architecture is defined in `docs/data-first-minimal-runtime.md`.

The only runtime integration in this repository is the `rquickjs` dependency
inside `crates/quench-node`. Do not introduce references, workspace members, or
tasks that depend on `quench-runtime`.

## Workflow

1. Select the next upstream Node fixture or API cluster.
2. Model the reusable API facts in the shared declaration/IR layer first.
3. Generate registration, wrappers, and ordinary tests; hand-write only
   irreducible compatibility behavior.
4. Add a focused stage under `tests/node-compat/stage-N/`, run it, format with
   Prettier, and run `git diff --check`.
5. Commit and push each verified stage before starting the next one.

Run a stage with:

```sh
cargo run -p quench-node -- --stage N
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
