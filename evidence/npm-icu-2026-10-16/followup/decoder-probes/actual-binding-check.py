from pathlib import Path
import subprocess,json,hashlib
p=Path('/workspace/scratch/npm-decoder-production-draft');engine=Path('/workspace/scratch/npm-icu-fixed-candidate')
assert hashlib.sha256(engine.read_bytes()).hexdigest()=='6488fa2069243903ac2ba1e3b6f7ca6d025cc9111c9d9aafb964bd44236b88f7'
for label,count in [('transcode-native-nonempty',8256),('width',131088)]:
 with (p/f'{label}-input.jsonl').open('rb') as src,(p/f'actual-{label}-quench.jsonl').open('wb') as out,(p/f'actual-{label}-quench.stderr').open('wb') as err:
  r=subprocess.run([str(engine),'--expose-internals',str(p/'native-oracle.cjs')],stdin=src,stdout=out,stderr=err,timeout=180)
 (p/f'actual-{label}.exit').write_text(str(r.returncode)+'\n')
 if r.returncode:raise RuntimeError(label+': '+(p/f'actual-{label}-quench.stderr').read_text()[:1000])
 left=[json.loads(s) for s in (p/f'{label}-node.jsonl').read_text().splitlines()]
 right=[json.loads(s) for s in (p/f'actual-{label}-quench.jsonl').read_text().splitlines()]
 assert len(left)==len(right)==count
 diff=[{'index':i,'node':a,'quench':b}for i,(a,b) in enumerate(zip(left,right))if a!=b]
 (p/f'actual-{label}-finding.json').write_text(json.dumps({'source':'383fcec1750a9eca4673853b2137d3b746c246d3','cli_sha256':hashlib.sha256(engine.read_bytes()).hexdigest(),'cases':count,'differences':len(diff),'rows':diff},indent=2)+'\n')
 print(label,count,len(diff),flush=True)
 if diff:raise RuntimeError(label+': oracle mismatch')
