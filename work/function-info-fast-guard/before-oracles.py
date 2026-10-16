import difflib,hashlib,json,subprocess
from pathlib import Path
out=Path('work/function-info-fast-guard');rows=[]
for name in ['test-function-constructor-debug-names','test-function-prototype-info-lookups']:
 source=Path('crates/quench-node-test/node-tests/'+name+'.js').read_text();results=[]
 for label,binary in [('node','work/node-v26.12.0-linux-x64/bin/node'),('before','work/function-info-before-bin/quench-node')]:
  r=subprocess.run([str(Path(binary).resolve()),'--expose-internals','--no-warnings','-e',source],capture_output=True,timeout=30)
  (out/(name+'-'+label+'.stdout')).write_bytes(r.stdout);(out/(name+'-'+label+'.stderr')).write_bytes(r.stderr)
  results.append(r);assert r.returncode==0 and not r.stderr,(name,label,r.returncode,r.stderr[:1000])
 assert results[0].stdout==results[1].stdout,name
 rows.append({'fixture':name,'both_exit':0,'stdout_sha256':hashlib.sha256(results[0].stdout).hexdigest(),'bytes':len(results[0].stdout),'equal':True})
(out/'before-oracle-receipt.json').write_text(json.dumps(rows,indent=2)+'\n');print(rows)
