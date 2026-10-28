#!/bin/bash
set -euo pipefail
cd /workspace/quench
export PATH=/workspace/quench/work/node-v26.12.0-linux-x64/bin:$PATH
unset QUENCH_NODE_LIBRARY_ROOT QUENCH_NODE_TEST_EXECUTABLE
python3 - <<'WAIT'
import json,time
from pathlib import Path
while True:
 s=json.loads(Path('work/task-supervisor/error-source-state.json').read_text())
 if s['status']!='running':
  assert s['status']=='complete' and s['exit']==0,s
  break
 time.sleep(5)
WAIT
python3 - <<'FREEZE'
from pathlib import Path
import hashlib,json,shutil,subprocess
b=Path('work/error-source-bin');assert not b.exists();b.mkdir()
for n in ['quench-node','run-parallel']:shutil.copy2(Path('target/iteration')/n,b/n)
shutil.copy2('work/prepared-realm-bin/openssl-cli',b/'openssl-cli')
files=subprocess.check_output(['git','ls-files','crates'],text=True).splitlines()
(b/'source-sha256.json').write_text(json.dumps({n:hashlib.sha256(Path(n).read_bytes()).hexdigest()for n in files if Path(n).is_file()},indent=2)+'\n')
(b/'source-commit.txt').write_text(subprocess.check_output(['git','rev-parse','HEAD'],text=True))
(b/'sha256.txt').write_text(''.join(hashlib.sha256((b/n).read_bytes()).hexdigest()+'  '+n+'\n'for n in ['quench-node','run-parallel','openssl-cli']))
FREEZE
node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/error-source-policy/shadow-node.log 2>&1
work/error-source-bin/quench-node --no-warnings --expose-internals --experimental-shadow-realm --expose-gc work/shadow-stack-isolation/oracle.cjs > work/error-source-policy/shadow-quench.log 2>&1
diff -u work/error-source-policy/shadow-node.log work/error-source-policy/shadow-quench.log > work/error-source-policy/shadow.diff
for n in oracle concat-oracle latin1-oracle poison-oracle; do
 work/error-source-bin/quench-node --no-warnings --expose-internals --expose-gc work/native-buffer-unsafe/$n.cjs > work/error-source-policy/buffer-$n.log 2>&1
 diff -u work/native-buffer-unsafe/$n.final-node.log work/error-source-policy/buffer-$n.log > work/error-source-policy/buffer-$n.diff
done
python3 - <<'FRAMEWORKS'
import json,subprocess
from pathlib import Path
rows=[]
for scenario in ['express','koa','fastify']:
 observations=[]
 for label,binary in [('node','work/node-v26.12.0-linux-x64/bin/node'),('before','work/shared-post-prepared-bin/quench-node'),('candidate','work/error-source-bin/quench-node')]:
  result=subprocess.run([str(Path(binary).resolve()),'tests/frameworks/driver.cjs','tests/frameworks/scenarios/'+scenario+'.cjs'],capture_output=True,text=True,timeout=30)
  observations.append({'engine':label,'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr})
 row={'scenario':scenario,'observations':observations,'matches':all((o['exit_code'],o['stdout'],o['stderr'])==(0,observations[0]['stdout'],'')for o in observations)}
 rows.append(row)
Path('work/error-source-policy/framework-scenarios.json').write_text(json.dumps(rows,indent=2)+'\n')
assert all(row['matches']for row in rows),rows
FRAMEWORKS
work/bench-tool-target/iteration/quench-bench --all --fixed-work --runs 1 --timeout-ms 300000 --quench /workspace/quench/work/shared-post-prepared-bin/quench-node --quench-peer /workspace/quench/work/error-source-bin/quench-node --out work/error-source-policy/fixed-work.json > work/error-source-policy/fixed-work.log 2>&1
python3 work/error-source-policy/family.py > work/error-source-policy/family-driver.log 2>&1
