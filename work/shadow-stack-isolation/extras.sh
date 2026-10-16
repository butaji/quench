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
 s=json.loads(Path('work/task-supervisor/shadow-stack-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
python3 - <<'FREEZE'
from pathlib import Path
import json,hashlib,shutil,subprocess
out=Path('work/shadow-stack-bin');out.mkdir(exist_ok=True)
for n in ['quench-node','run-parallel']:shutil.copy2(Path('target/iteration')/n,out/n)
shutil.copy2('work/native-buffer-unsafe-bin/openssl-cli',out/'openssl-cli')
keys=set(json.load(open('work/native-buffer-unsafe-bin/source-sha256.json')));keys.update(str(p)for p in Path('crates').rglob('*')if p.is_file()and p.suffix in ['.rs','.toml'])
(out/'source-sha256.json').write_text(json.dumps({n:hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in sorted(keys)},indent=2)+'\n')
(out/'sha256.txt').write_text(''.join(hashlib.sha256((out/n).read_bytes()).hexdigest()+'  '+n+'\n'for n in ['quench-node','run-parallel','openssl-cli']))
(out/'base-head.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
FREEZE
node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/shadow-stack-isolation/final-oracle.log 2>&1
work/shadow-stack-bin/quench-node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/shadow-stack-isolation/final-candidate.log 2>&1
diff -u work/shadow-stack-isolation/final-oracle.log work/shadow-stack-isolation/final-candidate.log > work/shadow-stack-isolation/final.diff
for n in oracle concat-oracle latin1-oracle poison-oracle; do
 work/shadow-stack-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$n.cjs > work/shadow-stack-isolation/buffer-$n.log 2>&1
 diff -u work/native-buffer-unsafe/$n.final-node.log work/shadow-stack-isolation/buffer-$n.log > work/shadow-stack-isolation/buffer-$n.diff
done
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/native-buffer-unsafe-bin/quench-node --quench-peer /workspace/quench/work/shadow-stack-bin/quench-node --out work/shadow-stack-isolation/fixed-work.json > work/shadow-stack-isolation/fixed-work.log 2>&1
export QUENCH_NODE_TEST_EXECUTABLE=/workspace/quench/work/shadow-stack-bin/quench-node
set +e
work/shadow-stack-bin/run-parallel --all --parallel-dir /workspace/work/node-stable-release/node-v26.12.0/test/parallel --filter test-stream-iter- --jobs 4 --timeout-secs 30 --results work/shadow-stack-isolation/stream-family.json > work/shadow-stack-isolation/stream-family.log 2>&1
code=$?
python3 - "$code" <<'CHECK'
import sys,json,collections
r=json.load(open('work/shadow-stack-isolation/stream-family.json'));c=collections.Counter(x['status']for x in r['results']);assert c.get('pass',0)>=52 and not c.get('fail',0),c;print('Real runner exit',sys.argv[1],dict(c))
CHECK
