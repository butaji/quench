#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'WAIT'
import hashlib,json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/prepared-full-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
base=Path('work/sdk-primitives-policy')
assert '660 passed; 0 failed'in(base/'core.log').read_text()
assert len(list(base.glob('focused-*.log')))==4
assert all('1 passed; 0 failed'in p.read_text()for p in base.glob('focused-*.log'))
m=json.loads((base/'qualified-source-sha256.json').read_text());assert all(hashlib.sha256(Path(n).read_bytes()).hexdigest()==h for n,h in m.items())
m=json.loads((base/'pre-remainder-binaries.json').read_text());assert all(hashlib.sha256((Path('target/iteration')/n).read_bytes()).hexdigest()==h for n,h in m.items())
WAIT
cargo test --profile iteration -p quench-node-test --test child_entry -j1 -- --test-threads=1 > work/sdk-primitives-policy/all-api.log 2>&1
target/iteration/run-parallel --profile framework-core > work/sdk-primitives-policy/framework.log 2>&1
cargo test --profile iteration -p quench-node-test --test shared_regressions -j1 -- --test-threads=1 > work/sdk-primitives-policy/shared.log 2>&1
cargo check --workspace --all-targets -j1 > work/sdk-primitives-policy/workspace.log 2>&1
