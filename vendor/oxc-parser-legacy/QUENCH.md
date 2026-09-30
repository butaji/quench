# Legacy OXC parser stack integration

Pinned OXC `oxc_parser` 0.47.1, archive SHA256
`08e83d2a08991efc1a79c3b42fcc77344f3de58d27c53cd9b184d17b1ed4ace3`,
upstream revision `8f5be07ed60cf4228dc94709fa0ae3f62096b704`, path `crates/oxc_parser`.
The upstream MIT license is retained. OXC owns grammar and syntax.

Recursive expression, statement and binding transitions share `quench-stack`.
Resource exhaustion is monotone parser state, survives syntax backtracking, and
is projected separately from syntax errors. `QUENCH.patch` records source changes
against the archive as a zero-context unified diff (`git apply --unidiff-zero`).
This transitional package is removed with the legacy compiler at task 27.

The archive's standalone Cargo.lock pins upstream test dependencies, including
the serde version required by OXC 0.47's regular-expression crate. Only the local
quench-stack dependency is added. Production dependency resolution uses the root
workspace lockfile. Run upstream tests with
`cargo test --manifest-path vendor/oxc-parser-legacy/Cargo.toml --lib`.
