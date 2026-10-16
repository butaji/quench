#!/bin/bash
set -euo pipefail
cd /workspace/quench
source /workspace/quench-build-env.sh
export CARGO_INCREMENTAL=0
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'WAIT'
import json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/buffer-integration-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
cargo test --profile iteration -p quench-runtime --test subset -j1 > work/native-buffer-integration/subset.log 2>&1
python3 - <<'FREEZE'
import json,hashlib,subprocess,shutil
from pathlib import Path
out=Path('work/native-buffer-integrated-bin');out.mkdir(exist_ok=True)
for name in ['quench-node','run-parallel']:shutil.copy2(Path('target/iteration')/name,out/name)
shutil.copy2('work/native-buffer-pool-bin/openssl-cli',out/'openssl-cli')
keys=set(json.load(open('work/native-buffer-pool-bin/source-sha256.json')))
keys.update(str(p)for p in Path('crates').rglob('*')if p.is_file()and p.suffix in ['.rs','.toml'])
(out/'source-sha256.json').write_text(json.dumps({n:hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in sorted(keys)},indent=2)+'\n')
(out/'source-commit.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
(out/'sha256.txt').write_text(''.join(hashlib.sha256((out/n).read_bytes()).hexdigest()+'  '+n+'\n'for n in ['quench-node','run-parallel','openssl-cli']))
FREEZE
work/native-buffer-integrated-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-allocation/oracle.cjs > work/native-buffer-integration/allocation.log 2>&1
diff -u work/native-buffer-allocation/accounted-oracle.log work/native-buffer-integration/allocation.log > work/native-buffer-integration/allocation.diff
work/native-buffer-integrated-bin/quench-node --no-warnings --experimental-stream-iter work/native-buffer-pool/oracle.cjs > work/native-buffer-integration/pool.log 2>&1
diff -u work/native-buffer-pool/accounted-oracle.log work/native-buffer-integration/pool.log > work/native-buffer-integration/pool.diff
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/native-buffer-pool-bin/quench-node --quench-peer /workspace/quench/work/native-buffer-integrated-bin/quench-node --out work/native-buffer-integration/fixed-work.json > work/native-buffer-integration/fixed-work.log 2>&1
set +e
work/native-buffer-integrated-bin/run-parallel --all --parallel-dir /workspace/work/node-stable-release/node-v26.12.0/test/parallel --filter test-stream-iter- --jobs 4 --timeout-secs 30 --results work/native-buffer-integration/family.json > work/native-buffer-integration/family.log 2>&1
code=$?
python3 - "$code" <<'CHECK'
import sys,json,collections
r=json.load(open('work/native-buffer-integration/family.json'));counts=collections.Counter(x['status']for x in r['results']);assert counts.get('pass',0)>=52 and not counts.get('fail',0),counts;print('Real runner exit',sys.argv[1],dict(counts))
CHECK
