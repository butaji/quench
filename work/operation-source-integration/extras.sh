#!/bin/bash
set -euo pipefail
cd /workspace/quench
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'WAIT'
import json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/operation-source-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/operation-source-integration/shadow-node.log 2>&1
work/operation-source-bin/quench-node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/operation-source-integration/shadow-quench.log 2>&1
diff -u work/operation-source-integration/shadow-node.log work/operation-source-integration/shadow-quench.log > work/operation-source-integration/shadow.diff
for n in oracle concat-oracle latin1-oracle poison-oracle; do
 work/operation-source-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$n.cjs > work/operation-source-integration/buffer-$n.log 2>&1
 diff -u work/native-buffer-unsafe/$n.final-node.log work/operation-source-integration/buffer-$n.log > work/operation-source-integration/buffer-$n.diff
done
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/shadow-stack-bin/quench-node --quench-peer /workspace/quench/work/operation-source-bin/quench-node --out work/operation-source-integration/fixed-work.json > work/operation-source-integration/fixed-work.log 2>&1
