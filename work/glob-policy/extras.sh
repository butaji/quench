#!/bin/bash
set -euo pipefail
cd /workspace/quench
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'WAIT'
import json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/glob-policy-quiet-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
python3 - <<'FREEZE'
from pathlib import Path
import hashlib,json,shutil,subprocess
b=Path('work/glob-policy-bin');b.mkdir(exist_ok=True)
for n in ['quench-node','run-parallel']:shutil.copy2(Path('target/iteration')/n,b/n)
shutil.copy2('work/operation-source-bin/openssl-cli',b/'openssl-cli')
files=subprocess.check_output(['git','ls-files','crates'],text=True).splitlines()
(b/'source-sha256.json').write_text(json.dumps({n:hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in files if Path(n).is_file()},indent=2)+'\n')
(b/'source-commit.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
(b/'sha256.txt').write_text(''.join(hashlib.sha256((b/n).read_bytes()).hexdigest()+'  '+n+'\n'for n in ['quench-node','run-parallel','openssl-cli']))
FREEZE
node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/glob-policy/shadow-node.log 2>&1
work/glob-policy-bin/quench-node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/glob-policy/shadow-quench.log 2>&1
diff -u work/glob-policy/shadow-node.log work/glob-policy/shadow-quench.log > work/glob-policy/shadow.diff
for n in oracle concat-oracle latin1-oracle poison-oracle; do
 work/glob-policy-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$n.cjs > work/glob-policy/buffer-$n.log 2>&1
 diff -u work/native-buffer-unsafe/$n.final-node.log work/glob-policy/buffer-$n.log > work/glob-policy/buffer-$n.diff
done
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/operation-source-bin/quench-node --quench-peer /workspace/quench/work/glob-policy-bin/quench-node --out work/glob-policy/fixed-work.json > work/glob-policy/fixed-work.log 2>&1

python3 work/glob-policy/family.py > work/glob-policy/family-driver.log 2>&1
