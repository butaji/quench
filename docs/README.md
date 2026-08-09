# Documentation

The repository [README](../README.md) contains the current Quench doctrine.
The test262 runner remains in `crates/quench-test262` and its supporting tools;
the execution engine is intentionally being rebuilt around the doctrine.

Run `tools/lint-rust.sh` before committing. It enforces zero warnings, the
500-line file limit, the 40-line function limit, and cognitive complexity 10.
