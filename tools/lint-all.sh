#!/usr/bin/env bash
# Run every repository source lint gate.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

tools/lint-js.sh
tools/lint-rs.sh
