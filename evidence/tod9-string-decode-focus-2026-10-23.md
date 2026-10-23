# TOD-9 string focus measurements

## Correctness checks

The iteration profile ran both focused runtime tests successfully:

```text
test vm::tests::empty_string_repeat_keeps_coercion_order_without_allocating_a_copy ... ok
test vm::tests::decode_uri_reuses_unescaped_input_and_preserves_uri_semantics ... ok
```

Commands used (with `CARGO_TARGET_DIR=/workspace/quench/target`,
`CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, and
`TMPDIR=/workspace/quench/target/tmp`):

```sh
cargo test --locked --profile iteration -p quench-runtime --lib empty_string_repeat -- --nocapture
cargo test --locked --profile iteration -p quench-runtime --lib decode_uri_reuses_unescaped_input -- --nocapture
```

The Node v24.19.0 oracle passed these same inputs: a single `valueOf` call for
`''.repeat(Number.MAX_SAFE_INTEGER)`, boxed empty strings, RangeErrors for
infinity and negative counts, a TypeError for a Symbol count, no-escape Unicode,
reserved escapes (`decodeURI('%E2%82%AC%3F') === '€%3F'` and
`decodeURIComponent(...) === '€?'`), and malformed, overlong, and surrogate
UTF-8 escapes throwing `URIError`.

## Timings

Platform: Linux x86_64, kernel `6.18.55`. Node was `v24.19.0`; Quench was built
with `cargo build --locked --profile iteration -p quench-node`, using
`CARGO_PROFILE_ITERATION_OPT_LEVEL=0`, 16 codegen units, and no JIT. Timings used
Bash's `TIMEFORMAT='%3R %3U %3S'` (`elapsed user system`) and process startup is
included.

The repeat case ran 10,000 calls to `''.repeat(2147483647)` and checked every
result, five separate process runs per engine.

| Runtime | elapsed seconds (five runs) |
| --- | --- |
| Node | 0.129, 0.068, 0.097, 0.054, 0.055 |
| Quench | 0.192, 0.067, 0.068, 0.067, 0.056 |

The decode case iterated valid four-byte UTF-8 sequences over the same codepoint
span as Test262 `S15.1.3.1_A2.5_T1.js`. Each `decodeURI` result was compared with
the expected UTF-16 surrogate pair. The script exited successfully in every run.

| Runtime | elapsed seconds (three runs) |
| --- | --- |
| Node | 0.277, 0.752, 8.807 |
| Quench | 42.776, 19.591, 18.596 |

These iteration-profile measurements have high host variance and include
unoptimized interpreter work. They are diagnostic only and cannot be compared
directly with the production-profile Test262 summary.
