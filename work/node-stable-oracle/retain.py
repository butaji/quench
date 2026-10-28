import collections,hashlib,json,re
from pathlib import Path
base=Path('work/node-stable-oracle');text=(base/'full.tap').read_text();receipt=json.loads((base/'receipt.json').read_text());assert receipt['changed_parallel_sources']==[] and receipt['protected_restored'];assert receipt['exit']==1
pattern=r'^(not ok|ok) (\d+) (.+)$';rows=re.findall(pattern,text,re.M);assert len(rows)==5012 and {int(n)for _,n,_ in rows}==set(range(1,5013))
counts=collections.Counter();results=[]
for kind,number,label in rows:
 name,sep,reason=label.partition(' # skip ')
 outcome='fail'if kind=='not ok'else'skip'if sep else'pass'
 counts[outcome]+=1;results.append({'case':Path(name).name,'status':outcome,**({'reason':reason}if sep else{})})
manifest=json.loads((base/'fixture-sha256.json').read_text());assert len(manifest)==5015
original=json.loads(Path('work/full-native-buffer-pool-stable/results.json').read_text());source={Path(r['fixture']).name:hashlib.sha256(Path(r['fixture']).read_bytes()).hexdigest()for r in original['results']};assert all(source[Path(n).name]==h for n,h in manifest.items())
selected={r['case']for r in results};omitted=sorted(n for n in source if Path(n).stem not in selected)
summary={'engine':'official Node v26.12.0','counts':dict(counts),'official_harness_total':len(rows),'all_parallel_source_count':5015,'source_stems_without_tap_label':omitted,'repeated_tap_labels':{n:c for n,c in collections.Counter(r['case']for r in results).items()if c>1},'failures':[r['case']for r in results if r['status']=='fail'],'source_bytes_equal':True,'elapsed_seconds':receipt['elapsed_seconds']}
(base/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');(base/'results.json').write_text(json.dumps(results,indent=2)+'\n')
def raw(p):
 data=p.read_bytes();return {'path':str(p),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
paths=[p for p in sorted(base.iterdir())if p.is_file()and p.name!='retention-summary.log'];assert hashlib.sha256(Path(receipt['command'][3]).read_bytes()).hexdigest()==receipt['node_sha256'];output={**summary,'receipt':receipt,'raw':[raw(p)for p in paths],'limitations':['Independent official-harness oracle baseline. Stock status exclusions and timeout multipliers differ from raw Quench5015; do not combine these scores.','TAP labels omit extensions and can repeat; no one-to-one raw-inventory membership claim. All17 actual official failures and all skips retained. No Quench failure is excused or converted to a pass by this baseline.','Two package-boundary-invalid attempts were stopped and retained with actual exit -15; only final outside-package full run supplies this result.','Compilation and Quench pinned/stable audits overlapped; no speed claim.']}
Path('evidence/task20-node-latest-stable-official-full-control-2026-10-16.json').write_text(json.dumps(output,indent=2)+'\n');print(json.dumps(summary))
