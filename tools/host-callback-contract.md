# Host callback contract

Use this template for every new `__quench_*` Rust callback. Keep the callback
small and move compatibility behavior into the JavaScript caller.

## Identity

- Name: `__quench_...`
- Stage: `stage-N`
- Owner task: `tasks/...`
- Upstream fixture: `tests/node/test/parallel/...`

## Signature

- Arguments: `...`
- Return value: `...`
- Synchronous or asynchronous: `...`
- Error representation: thrown exception, error value, or callback argument

## Behavior

- Preconditions and validation: `...`
- Side effects: `...`
- Resource ownership and cleanup: `...`
- Platform-specific behavior: `...`

## Verification

- Focused stage command: `cargo run -q -p quench-node -- --stage N`
- Upstream fixture command: `tools/run-node-tests.sh ...`
- Rust checks: `tools/lint-rs.sh` and `cargo nextest run -p quench-node`
- Retrospective: `...`
