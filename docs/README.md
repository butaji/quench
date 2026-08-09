# Documentation

The repository [README](../README.md) states the Quench doctrine.
[ADR 0005](adr/0005-oxc-facts-residual-vm.md) is the architectural authority;
[the architecture](architecture.md) explains its execution path and invariants.
Superseded ADRs are historical records, not implementation options.

```bash
cargo build -p quench-runtime
cargo test -p quench-runtime --test test262 test262_staged -- --ignored --nocapture
```

`tasks/implementation-plan.md` is the migration gate. `tasks/index.json`
records only stage identity and workflow status. Run the stage to establish
conformance; do not treat prose or saved reports as status.
