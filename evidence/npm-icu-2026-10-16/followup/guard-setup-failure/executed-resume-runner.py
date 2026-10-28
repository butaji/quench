from pathlib import Path
import subprocess,os,json,hashlib
s=Path('/workspace/scratch');p='npm-icu-fixed';root=Path('/workspace/quench');guard=root/'target/npm-icu-fixed-instruction-guard'
def receipt(label,value): (s/f'{p}-{label}.exit').write_text(str(value)+'\n')
try:
 engines=json.loads((s/f'{p}-engines.json').read_text())
 for path,sha in [(s/'npm-icu-fixed-candidate',engines['candidate']['sha256']),(s/'npm-functions-candidate',engines['baseline']['sha256']),('/workspace/shared/node-v26.12.0-linux-x64/bin/node',engines['node']['sha256'])]:
  assert hashlib.sha256(Path(path).read_bytes()).hexdigest()==sha
 assert engines['candidate']['source']=='383fcec1750a9eca4673853b2137d3b746c246d3'
 for label in ['build-checks','fast-core','opt-core','node-api','node-lib','workspace-check','portable-check','cli','runners','coverage-unit']:
  assert (s/f'{p}-{label}.exit').read_text().strip()=='0',label
 for label in ['scenarios','baseline-scenarios','surface']:
  assert len(json.loads((s/f'{p}-{label}.json').read_text())['records'])==300
 env=os.environ.copy();env['PATH']='/workspace/.local/usr/bin:'+env['PATH'];env['VALGRIND_LIB']='/workspace/.local/usr/libexec/valgrind'
 (s/f'{p}-guard-resume-provenance.json').write_text(json.dumps({'source':engines['candidate']['source'],'resumed_after':'missing-valgrind-PATH setup failure; no production or functional-harness changes','engines':engines,'PATH':env['PATH'],'VALGRIND_LIB':env['VALGRIND_LIB']},indent=2)+'\n')
 with (s/f'{p}-instruction-guard.log').open('wb') as out:
  r=subprocess.run(['bash',str(s/f'{p}-callgrind-guard.sh'),str(s/f'{p}-candidate'),str(s/f'{p}-candidate'),str(guard)],cwd=root,env=env,stdout=out,stderr=subprocess.STDOUT) if False else subprocess.run(['bash',str(s/f'{p}-callgrind-guard.sh'),str(s/'npm-functions-candidate'),str(s/f'{p}-candidate'),str(guard)],cwd=root,env=env,stdout=out,stderr=subprocess.STDOUT)
 receipt('instruction-guard',r.returncode);assert r.returncode==0,'instruction guard failed'
 rows=[]
 for side,engine in [('base',s/'npm-functions-candidate'),('cand',s/f'{p}-candidate')]:
  for script in sorted(guard.glob('*.js')):
   fixture,work=script.stem.rsplit('-',1);r=subprocess.run([str(engine),str(script)],capture_output=True,text=True,timeout=120)
   rows.append({'side':side,'script':script.name,'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr,'passed':r.returncode==0 and r.stdout==f'done {fixture} {work}\n' and r.stderr==''})
 (s/f'{p}-guard-completions.json').write_text(json.dumps(rows,indent=2)+'\n');assert len(rows)==32 and all(r['passed']for r in rows)
except Exception as error:
 (s/f'{p}-guard-resume-error.txt').write_text(repr(error)+'\n');receipt('guard-resume',1);raise
else:
 receipt('guard-resume',0);receipt('suite',0)
