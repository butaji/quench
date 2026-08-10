#!/usr/bin/env bash
# Enforce formatting and complexity/size limits on all project JS files
# (outside vendored sources).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

echo "=== deno fmt --check ==="
deno fmt --check crates tests/node-compat tools AGENTS.md README.md package.json eslint.config.js || {
  echo "FAIL: deno fmt — run 'deno fmt crates tests/node-compat tools AGENTS.md README.md package.json eslint.config.js'" >&2
  exit 1
}
echo "OK: deno fmt"

tools/lint-size.sh

echo ""
echo "=== eslint (size + complexity) ==="
npx --yes eslint --max-warnings=0 'crates/**/*.js' 'tests/node-compat/**/*.{js,mjs}' 'eslint.config.js'
echo "OK: eslint"

echo ""
echo "All JS checks passed."
