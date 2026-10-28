import collections,hashlib,json,sys
from pathlib import Path
release=sys.argv[1];assert release in ['stable','pinned']
base=Path('work/full-sdk-primitives-'+release);before=Path('work/full-prepared-realm-glob-'+release)
receipt=json.loads((base/'receipt.json').read_text());data=json.loads((base/'results.json').read_text());old=json.loads((before/'results.json').read_text());baseline=json.loads((before/'summary.json').read_text())
assert receipt['source_commit']=='3218431de1e87a51b63fee8bd3091aa631eec046'
assert not receipt['changed_sources_after'] and receipt['binaries_before']==receipt['binaries_after'] and receipt['protected_after']['restored']
a={Path(r['fixture']).name:r['status']for r in old['results']};b={Path(r['fixture']).name:r['status']for r in data['results']};assert a.keys()==b.keys() and data['inventory_hash']==old['inventory_hash']
counts=dict(collections.Counter(b.values()));comparison={'gained':[n for n in sorted(b)if b[n]=='pass'and a[n]!='pass'],'lost':[n for n in sorted(b)if b[n]!='pass'and a[n]=='pass'],'skip_changes':{n:{'before':a[n],'after':b[n]}for n in sorted(b)if a[n]!=b[n]and'skip'in[a[n],b[n]]}}
(base/'comparison.json').write_text(json.dumps(comparison,indent=2)+'\n')
def events(s):return {k:int(v)for k,v in(line.split()for line in s.splitlines())}
previous=events(receipt['memory_before']);current=events(receipt['memory_after']);delta={k:current[k]-v for k,v in previous.items()}
def raw(p):
 content=p.read_bytes();return {'path':str(p),'bytes':len(content),'sha256':hashlib.sha256(content).hexdigest()}
paths=[base/n for n in ['results.json','runner.log','receipt-before.json','receipt.json','summary.json','comparison.json']]
paths += sorted(p for p in base.iterdir()if p.is_file()and('control'in p.name))
output={'scope':'Complete unchanged '+release+' parallel inventory after genuine SDK error/caller/protocol bindings and deferred promises glob policy','source_commit':receipt['source_commit'],'release':receipt['release'],'fixture_commit':receipt['fixture_commit'],'counts':counts,'total':len(b),'inventory_hash':data['inventory_hash'],'baseline':baseline,**comparison,'memory_events_delta':delta,'elapsed_seconds':receipt['elapsed_seconds'],'raw':[raw(p)for p in paths],'limitations':['Goal remains open; all nonpass outcomes retained. No selected-case sums, skips/deadline/budget changes or speed claim.','This frozen321 run includes SDK primitives but excludes subsequent function-info/atom-guard/owned-Script work.','Native compilation overlapped part of this run. Actual memory.events deltas are retained below; concurrent resource pressure is not subtracted from outcomes. All full outcomes retained, no speed claim or adjusted totals.']}
Path('evidence/task20-node-'+release+'-sdk-primitives-full-2026-10-16.json').write_text(json.dumps(output,indent=2)+'\n')
print(json.dumps({'counts':counts,**comparison,'memory_events_delta':delta}))
