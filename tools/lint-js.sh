#!/usr/bin/env bash
# Enforce formatting and complexity/size limits on all project JS files
# (outside vendored sources).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

tools/lint-size.sh

echo "=== prettier --check ==="
npx --yes prettier --check 'crates/**/*.js' 'tests/node-compat/**/*.js' || {
  echo "FAIL: prettier — run 'npx prettier --write ..." >&2
  exit 1
}
echo "OK: prettier"

echo ""
echo "=== eslint (size + complexity) ==="
npx --yes eslint 'crates/**/*.js' 'tests/node-compat/**/*.js' 'eslint.config.js'
echo "OK: eslint"

echo ""
echo "All JS checks passed."
