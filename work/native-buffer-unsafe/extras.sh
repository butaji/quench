#!/bin/bash
set -euo pipefail
cd /workspace/quench
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
for name in oracle concat-oracle latin1-oracle poison-oracle; do
 node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$name.cjs > work/native-buffer-unsafe/$name.final-node.log 2>&1
 work/native-buffer-unsafe-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$name.cjs > work/native-buffer-unsafe/$name.final-candidate.log 2>&1
 diff -u work/native-buffer-unsafe/$name.final-node.log work/native-buffer-unsafe/$name.final-candidate.log > work/native-buffer-unsafe/$name.final.diff
done
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/native-buffer-integrated-bin/quench-node --quench-peer /workspace/quench/work/native-buffer-unsafe-bin/quench-node --out work/native-buffer-unsafe/fixed-work.json > work/native-buffer-unsafe/fixed-work.log 2>&1
export QUENCH_NODE_TEST_EXECUTABLE=/workspace/quench/work/native-buffer-unsafe-bin/quench-node
for name in test-buffer-alloc-alignment.js test-buffer-islatin1.js test-buffer-concat.js; do
 work/native-buffer-unsafe-bin/run-parallel --all --parallel-dir /workspace/work/node-stable-release/node-v26.12.0/test/parallel --filter "$name" --jobs 1 --timeout-secs 30 --results work/native-buffer-unsafe/$name.results.json > work/native-buffer-unsafe/$name.log 2>&1
done
set +e
work/native-buffer-unsafe-bin/run-parallel --all --parallel-dir /workspace/work/node-stable-release/node-v26.12.0/test/parallel --filter test-stream-iter- --jobs 4 --timeout-secs 30 --results work/native-buffer-unsafe/family.json > work/native-buffer-unsafe/family.log 2>&1
code=$?
python3 - "$code" <<'CHECK'
import json,sys,collections
r=json.load(open('work/native-buffer-unsafe/family.json'));c=collections.Counter(x['status']for x in r['results']);assert c.get('pass',0)>=52 and not c.get('fail',0),c;print('Real runner exit',sys.argv[1],dict(c))
CHECK
