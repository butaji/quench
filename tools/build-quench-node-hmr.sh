#!/usr/bin/env bash
set -euo pipefail

# TSRS has an isolated lockfile because its pinned dependency graph conflicts with
# Quench's OXC lockfile. Build both artifacts into the locations HMR discovers.
cargo build --locked --manifest-path tools/quench-tsrs-worker/Cargo.toml
cargo build -p quench-node
