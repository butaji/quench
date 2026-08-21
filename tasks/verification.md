# Verification and delivery

## Required evidence

Green and yellow status MUST be based on executable Node API tests, not module
registration, focused fixtures, or documentation claims alone.

Required commands:

```sh
cargo test --workspace
cargo run -p quench-node-test --bin run-compat -- --quiet
cargo run -p quench-node-test --bin run-parallel
```

`run-compat` is the repository's focused API suite. `run-parallel` executes the
upstream Node parallel fixtures listed in
`crates/quench-node-test/node-tests/parallel.txt`. A task cannot be marked
verified until the relevant green/yellow fixtures pass and the upstream run
completes without a panic or unclassified failure.

## Per-implementation definition of done

Every implementation task MUST name the related Node API fixture(s), the
focused command used to execute them, and the applicable upstream Node test
command. A task is not done when code compiles or a namespace exists. It is
done only when behavior is exercised, results are recorded, and every
remaining failure or unsupported/platform-limited case is listed as a gap.

## Current measured status

Measured after the latest compatibility fixes:

- `cargo check -p quench-node`: passes, with one existing unused-variable
  warning.
- `cargo run -p quench-node-test --bin run-compat -- --quiet`: **67 passed,
  0 failed, 67 total**.
- `cargo run -p quench-node-test --bin run-parallel`: **178 passed, 0 failed,
  178 total**.
- Focused diagnostics_channel, inspector, repl/wasi, DNS, path, events,
  HTTP, net, readline, and WASI option fixtures pass individually.

The previous datetime-format panic is fixed and covered by a regression unit
test. The focused and upstream suites are green for their current manifests.
This is evidence for the repository's tested Node API set; it is not a claim
that every Bun-documented Node v26 API is implemented.

Residual gap policy:

- Bun yellow modules remain partial unless their documented gaps are either
  implemented or explicitly recorded.
- Bun green modules outside the current manifests still require applicable
  upstream Node API fixtures.
- Every new implementation MUST add or identify its related Node API test and
  record the command/result before status changes to verified.

## Completion rule

The compatibility matrix is complete only when every listed green/yellow
module has:

1. an implementation or an explicit unsupported/platform-limited status;
2. a focused fixture;
3. applicable upstream Node API test evidence;
4. recorded pass/fail/unsupported results and reproduction commands.

Rust lint is a separate quality gate. It MUST be reported independently and
MUST NOT substitute for Node API behavior verification.