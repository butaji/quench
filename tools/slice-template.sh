#!/usr/bin/env bash
# Usage: tools/slice-template.sh <stage_num> <test_name> [module_prefix]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <stage_num> <test_name> [module_prefix]" >&2
  exit 1
fi

stage="$1"
name="$2"
module="${3:-}"

outdir="$root/tests/node-compat/stage-${stage}"
mkdir -p "$outdir"

file="$outdir/${name}.js"

cat > "$file" <<'END_TEMPLATE'
// Stage-driven compatibility test for Node.js behaviour.
'use strict';
const common = require('../common');
const assert = require('assert');

// Place test assertions here. Each test exercises one contract point.
// Use `common.mustCall` or `common.mustSucceed` where async callbacks exist.

// Example:
// const result = require('???').someMethod('arg');
// assert.strictEqual(result, expected);

END_TEMPLATE

# If a module prefix is given, seed a quick require stub comment
if [[ -n "$module" ]]; then
  sed -i '' "s|require('???')|require('${module}')|" "$file"
fi

# Print a one-line summary
echo "Created stage ${stage}/${name}.js in ${outdir}"
echo "Run: cargo run -q --manifest-path \"$root/Cargo.toml\" -p quench-node -- --stage $stage"
