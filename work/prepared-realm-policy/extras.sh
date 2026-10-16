#!/bin/bash
set -euo pipefail
cd /workspace/quench
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'WAIT'
import json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/prepared-realm-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
python3 - <<'FREEZE'
from pathlib import Path
import hashlib,json,shutil,subprocess
b=Path('work/prepared-realm-bin');b.mkdir(exist_ok=True)
for n in ['quench-node','run-parallel']:shutil.copy2(Path('target/iteration')/n,b/n)
shutil.copy2('work/operation-source-bin/openssl-cli',b/'openssl-cli')
files=subprocess.check_output(['git','ls-files','crates'],text=True).splitlines()
(b/'source-sha256.json').write_text(json.dumps({n:hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in files if Path(n).is_file()},indent=2)+'\n')
(b/'source-commit.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
(b/'sha256.txt').write_text(''.join(hashlib.sha256((b/n).read_bytes()).hexdigest()+'  '+n+'\n'for n in ['quench-node','run-parallel','openssl-cli']))
FREEZE
node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/prepared-realm-policy/shadow-node.log 2>&1
work/prepared-realm-bin/quench-node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/prepared-realm-policy/shadow-quench.log 2>&1
diff -u work/prepared-realm-policy/shadow-node.log work/prepared-realm-policy/shadow-quench.log > work/prepared-realm-policy/shadow.diff
for n in oracle concat-oracle latin1-oracle poison-oracle; do
 work/prepared-realm-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$n.cjs > work/prepared-realm-policy/buffer-$n.log 2>&1
 diff -u work/native-buffer-unsafe/$n.final-node.log work/prepared-realm-policy/buffer-$n.log > work/prepared-realm-policy/buffer-$n.diff
done
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/glob-policy-bin/quench-node --quench-peer /workspace/quench/work/prepared-realm-bin/quench-node --out work/prepared-realm-policy/fixed-work.json > work/prepared-realm-policy/fixed-work.log 2>&1

python3 work/prepared-realm-policy/family.py > work/prepared-realm-policy/family-driver.log 2>&1
python3 work/prepared-realm-policy/loss-controls.py > work/prepared-realm-policy/loss-controls.log 2>&1
