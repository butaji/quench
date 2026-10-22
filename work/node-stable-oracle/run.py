import datetime,hashlib,json,os,subprocess,time
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/node-stable-oracle';source=Path('/workspace/work/node-stable-release/node-v26.12.0');fixtures=Path('/workspace/work/node-stable-oracle-copy/test');node=root/'work/node-v26.12.0-linux-x64/bin/node'
assert subprocess.check_output([str(node),'--version'],text=True).strip()=='v26.12.0'
files=sorted((fixtures/'parallel').glob('test-*.js'))+sorted((fixtures/'parallel').glob('test-*.mjs'))
manifest={str(p.relative_to(fixtures)):hashlib.sha256(p.read_bytes()).hexdigest()for p in files}
(out/'fixture-sha256.json').write_text(json.dumps(manifest,indent=2)+'\n')
protected=fixtures/'fixtures/permission/deny/protected-file.md';before=protected.read_bytes()
env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);env.pop('QUENCH_NODE_TEST_EXECUTABLE',None);env['PATH']=str(root/'work/apachebench-root/usr/bin')+':'+str(node.parent)+':'+env['PATH'];env['LD_LIBRARY_PATH']=str(root/'work/apachebench-root/usr/lib/x86_64-linux-gnu')+(':'+env['LD_LIBRARY_PATH']if env.get('LD_LIBRARY_PATH')else'')
command=['python3',str(source/'tools/test.py'),'--shell',str(node),'--test-root',str(fixtures),'--mode','release','--timeout','30','-j','4','-p','tap','parallel']
receipt={'engine':'official Node v26.12.0','node_sha256':hashlib.sha256(node.read_bytes()).hexdigest(),'runner':'unchanged official Node tools/test.py and stock platform status rules','command':command,'isolated_test_root':str(fixtures),'cwd':str(fixtures.parent),'package_boundary':'Pristine test copy outside Quench package scope; upstream default module detection, no added package type','parallel_source_count':len(manifest),'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'limitations':['Official harness selection/status rules and timeout multipliers differ from the raw Quench inventory; retain results as an independent oracle baseline, not a replacement Quench score.']}
(out/'receipt-before.json').write_text(json.dumps(receipt,indent=2)+'\n');started=time.monotonic()
try:
 with (out/'full.tap').open('w')as log:receipt['exit']=subprocess.run(command,cwd=fixtures.parent,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
finally:
 protected.write_bytes(before);receipt['protected_restored']=protected.read_bytes()==before;receipt['elapsed_seconds']=time.monotonic()-started;receipt['finished_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat();receipt['changed_parallel_sources']=[p for p,h in manifest.items()if hashlib.sha256((fixtures/p).read_bytes()).hexdigest()!=h];(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
