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

## Current measured status

Measured after the latest implementation and merge:

- `cargo check -p quench-node`: passes, with existing warnings.
- `cargo run -p quench-node-test --bin run-compat -- --quiet`: **49 passed,
  8 failed, 57 total**.
- `cargo run -p quench-node-test --bin run-parallel`: **blocked** by a runtime
  panic in `crates/quench-runtime/src/intl/datetime_format_date.rs:90`
  (`index out of bounds`); no upstream pass rate is claimable.
- Focused fixtures for diagnostics_channel, inspector, repl/wasi, dns, and
  path pass individually.

Current focused failures:

```text
events
http-client
http-keepalive
http-post
http-url
http
net
readline
```

Therefore green and yellow are **implemented in part**, but neither category
has full Node API verification. Existing task status text MUST NOT claim
`57/57`, `178/178`, or complete green/yellow coverage until these results are
reproduced successfully.

## Completion rule

The compatibility matrix is complete only when every listed green/yellow
module has:

1. an implementation or an explicit unsupported/platform-limited status;
2. a focused fixture;
3. applicable upstream Node API test evidence;
4. recorded pass/fail/unsupported results and reproduction commands.

Rust lint is a separate quality gate. It MUST be reported independently and
MUST NOT substitute for Node API behavior verification.