# Stage 09 — Node API suite acceptance

## Contract
`tests/node` is the sole oracle for Node API compatibility. Run applicable upstream parallel, ES-module, common, and fixture tests through `quench-node-test`.

## Required gates
1. Synchronize declarations, registry, wrappers, builtin inventories, dispatch, and callers.
2. Run `cargo run -p quench-node-test --bin run-compat -- --quiet`.
3. Run `cargo run -p quench-node-test --bin run-parallel`.
4. Classify every Node API failure and skip; never suppress results.

## Exit criteria
Every applicable Node API test passes with no unclassified failures or skips. WPT, Test262, npm consumer, benchmark, and release evidence are outside this task scope.
