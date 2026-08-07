#!/usr/bin/env sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
stage_dirs=$(find "$root/tests/node-compat" -mindepth 1 -maxdepth 1 -type d -name 'stage-*' | wc -l | tr -d ' ')
stage_files=$(find "$root/tests/node-compat" -mindepth 2 -type f \( -name '*.js' -o -name '*.mjs' \) | wc -l | tr -d ' ')
upstream_files=$(find "$root/tests/node/test" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) | wc -l | tr -d ' ')
upstream_parallel=$(find "$root/tests/node/test/parallel" -type f \( -name '*.js' -o -name '*.mjs' -o -name '*.cjs' \) | wc -l | tr -d ' ')
inventory="$root/target/compat/inventory.json"

echo "focused_stage_directories=$stage_dirs"
echo "focused_stage_files=$stage_files"
echo "upstream_test_files=$upstream_files"
echo "upstream_parallel_files=$upstream_parallel"
echo "node_api_coverage=unmeasured"
if [ -f "$inventory" ]; then
  QUENCH_COMPAT_INVENTORY="$inventory" node - <<'NODE'
const fs = require("fs");
const report = JSON.parse(fs.readFileSync(process.env.QUENCH_COMPAT_INVENTORY, "utf8"));
const modules = report.modules || {};
const globals = report.globals || {};
const canonical = modules.canonical?.length || 0;
const available = modules.runtimeAvailable?.length || 0;
const nodeGlobals = globals.node?.length || 0;
const assignedGlobals = globals.assignedByPolyfills?.length || 0;
console.log(`module_runtime_availability=${available}/${canonical}`);
console.log(`global_assignment_count=${assignedGlobals}`);
console.log(`node_global_surface_count=${nodeGlobals}`);
NODE
else
  echo "module_runtime_availability=unavailable"
  echo "global_assignment_observation=unavailable"
fi
echo "note=focused stages are contract gates, not a percentage of the Node API surface"
echo "test_file_rate_command=tools/measure-node-tests.sh [directory]"
