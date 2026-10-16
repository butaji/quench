#!/bin/bash
set -euo pipefail
cd /workspace/quench
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
python3 - <<'WAIT'
import json,time
from pathlib import Path
p=Path('work/task-supervisor/buffer-qualify-state.json')
while True:
 s=json.loads(p.read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
python3 - <<'FREEZE'
from pathlib import Path
import json,hashlib,shutil,subprocess
out=Path('work/native-buffer-pool-bin');out.mkdir(exist_ok=True)
for n in ['quench-node','run-parallel']:
 shutil.copy2(Path('target/iteration')/n,out/n)
# Existing actual OpenSSL companion is unchanged and independently verified.
shutil.copy2(Path('work/node-error-stack-brand-bin/openssl-cli'),out/'openssl-cli')
keys=set(json.load(open('work/node-error-stack-brand-bin/source-sha256.json')))
for p in Path('crates').rglob('*'):
 if p.is_file() and p.suffix in ['.rs','.toml']:keys.add(str(p))
manifest={k:hashlib.sha256(Path(k).read_bytes()).hexdigest() for k in sorted(keys)}
(out/'source-sha256.json').write_text(json.dumps(manifest,indent=2)+'\n')
(out/'sha256.txt').write_text(''.join(hashlib.sha256((out/n).read_bytes()).hexdigest()+'  '+n+'\n' for n in ['quench-node','run-parallel','openssl-cli']))
(out/'base-head.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
FREEZE
node --no-warnings --expose-internals --expose-gc work/native-buffer-allocation/oracle.cjs > work/native-buffer-allocation/accounted-oracle.log 2>&1
work/native-buffer-pool-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-allocation/oracle.cjs > work/native-buffer-allocation/accounted-candidate.log 2>&1
diff -u work/native-buffer-allocation/accounted-oracle.log work/native-buffer-allocation/accounted-candidate.log > work/native-buffer-allocation/accounted-diff.log
node --no-warnings --experimental-stream-iter work/native-buffer-pool/oracle.cjs > work/native-buffer-pool/accounted-oracle.log 2>&1
work/native-buffer-pool-bin/quench-node --no-warnings --experimental-stream-iter work/native-buffer-pool/oracle.cjs > work/native-buffer-pool/accounted-candidate.log 2>&1
diff -u work/native-buffer-pool/accounted-oracle.log work/native-buffer-pool/accounted-candidate.log > work/native-buffer-pool/accounted-diff.log
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/node-error-stack-brand-bin/quench-node --quench-peer /workspace/quench/work/native-buffer-pool-bin/quench-node --out work/native-buffer-pool-fixed-work.json > work/native-buffer-pool-fixed-work.log 2>&1
export QUENCH_NODE_TEST_EXECUTABLE=/workspace/quench/work/native-buffer-pool-bin/quench-node
set +e
work/native-buffer-pool-bin/run-parallel --all --parallel-dir /workspace/work/node-stable-release/node-v26.12.0/test/parallel --filter test-stream-iter- --jobs 4 --timeout-secs 30 --results work/native-buffer-pool/accounted-family.json > work/native-buffer-pool/accounted-family.log 2>&1
code=$?
python3 - "$code" <<'CHECK'
import sys,json,collections
r=json.load(open('work/native-buffer-pool/accounted-family.json'));c=collections.Counter(x['status'] for x in r['results']);assert c=={'pass':52,'timeout':1},c;print('Real runner exit',sys.argv[1],dict(c))
CHECK
