# Post-holder fixture samples

These are read-only samples from the symbolized production-profile build on
Apple M4/macOS 26.5. The binary is pinned at
`target/pinned/bc721076a739599ae8e8d76a453171a76fae3e5fadd09a771953ff1f2f076361/quench-node`
with SHA-256 `bc721076a739599ae8e8d76a453171a76fae3e5fadd09a771953ff1f2f076361`.
It was built from commit `cbb72f34aa3a8cd15ca8ce8c68f32d9b8650b393` with
production settings, line tables, and no profile counters.

The materialized inputs are hash-pinned in the files here. EarleyBoyer's
largest active stacks include `run_frame_general_until` (203 leaf samples),
`collect_slow` (72), `outer_environment_binding` (56),
`root_local_var_binding` (42), `active_shape_attributes` (35), `name_binding`
(33), `capture` (28), and construction (`construct_value` about 120 inclusive
samples). RegExp's active profile contains 1,058 samples under
`string_split_native`, 748 under `regexp_symbol_split`, and 440 under
`regexp_exec_value`; `Regex::find_from_utf16` accounts for 243 leaf samples.
Sample counts are attribution, not runtime shares or performance claims.

Files:

- `earley-boyer-sample.txt` and `earley-boyer-run.log`
- `regexp-sample.txt` and `regexp-run.log`
- `production-symbolized-build.log`
- `materialized-earley-boyer.js` and `materialized-regexp.js`
