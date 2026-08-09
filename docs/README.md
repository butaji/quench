# Quench

Quench is a Rust ECMAScript runtime pursuing 100% of the pinned ECMA-262
test262 suite (excluding `intl402` and `staging`). Its sole implementation
plan is [the architecture](architecture.md): OXC program data + unified facts
+ partial evaluation + residual operations + a compact heap.

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

`tasks/index.json` records only stage identity and workflow status. Run the
stage to establish conformance; do not treat prose or saved reports as status.
