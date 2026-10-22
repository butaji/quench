import datetime,hashlib,json,os,subprocess,time
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/node-stable-oracle';before=json.loads((out/'receipt.json').read_text());engine=Path('/workspace/work/node-stable-oracle-engine/bin/node');engine.parent.mkdir(parents=True,exist_ok=True)
source=root/'work/node-v26.12.0-linux-x64/bin/node'
if not engine.exists():os.link(source,engine)
assert hashlib.sha256(engine.read_bytes()).hexdigest()==before['node_sha256']
names=json.loads((out/'summary.json').read_text())['failures'];fixtures=Path('/workspace/work/node-stable-oracle-copy/test');command=['python3','/workspace/work/node-stable-release/node-v26.12.0/tools/test.py','--shell',str(engine),'--test-root',str(fixtures),'--mode','release','--timeout','30','-j','4','-p','tap']+['parallel/'+name for name in names]
env=os.environ.copy();env['PATH']=str(root/'work/apachebench-root/usr/bin')+':'+str(engine.parent)+':'+env['PATH'];env['LD_LIBRARY_PATH']=str(root/'work/apachebench-root/usr/lib/x86_64-linux-gnu')+(':'+env['LD_LIBRARY_PATH']if env.get('LD_LIBRARY_PATH')else'')
receipt={'purpose':'Focused controls for all17 official-baseline failures using byte-identical executable outside Quench package scope','command':command,'node_sha256':before['node_sha256'],'cwd':str(fixtures.parent),'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat()};started=time.monotonic()
with (out/'exec-outside-control.tap').open('w')as log:receipt['exit']=subprocess.run(command,cwd=fixtures.parent,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
receipt['elapsed_seconds']=time.monotonic()-started;receipt['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat();receipt['binary_unchanged']=hashlib.sha256(engine.read_bytes()).hexdigest()==before['node_sha256'];(out/'exec-outside-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
