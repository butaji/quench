#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
stage_dirs=$(find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' | wc -l | tr -d ' ')
stage_files=$(find "$root/tests/node-compat" -mindepth 2 -type f -name '*.js' | wc -l | tr -d ' ')
upstream_files=$(find "$root/tests/node/test" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) | wc -l | tr -d ' ')
upstream_parallel=$(find "$root/tests/node/test/parallel" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) | wc -l | tr -d ' ')

echo "focused_stage_directories=$stage_dirs"
echo "focused_stage_files=$stage_files"
echo "upstream_test_files=$upstream_files"
echo "upstream_parallel_files=$upstream_parallel"
echo "node_api_coverage=unmeasured"
echo "note=focused stages are contract gates, not a percentage of the Node API surface"
echo "test_file_rate_command=tools/measure-node-tests.sh [directory]"
