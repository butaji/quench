#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'CHECK'
from pathlib import Path
import json,hashlib
for path in ['work/glob-policy/core655-source-sha256.json','work/glob-policy/quiet-source-sha256.json']:
 m=json.loads(Path(path).read_text());assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in m.items())
assert '655 passed; 0 failed' in Path('work/glob-policy/core.log').read_text()
CHECK
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/glob-policy/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/glob-policy/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/glob-policy/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/glob-policy/workspace.log 2>&1
