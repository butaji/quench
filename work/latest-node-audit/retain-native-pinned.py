import json,hashlib,collections
from pathlib import Path
base=Path('work/full-native-codecs-restart-pinned');r=json.loads((base/'receipt.json').read_text());d=json.loads((base/'results.json').read_text());c=json.loads((base/'comparison.json').read_text());controls=json.loads((base/'lost-controls.json').read_text())
counts=dict(collections.Counter(x['status']for x in d['results']));assert counts=={'pass':1809,'skip':677,'fail':2159,'timeout':82},counts
assert len(d['results'])==4727 and d['inventory_hash']=='1e51ca4121020d14'
assert len(c['gained'])==12 and len(c['lost'])==2 and not c['new_skips']
assert r['binaries_before']==r['binaries_after'] and not r['changed_sources_after'] and r['protected_before']==r['protected_after']
def events(s):return dict((k,int(v))for k,v in map(str.split,s.splitlines()))
a,b=events(r['memory_before']),events(r['memory_after']);delta={k:b[k]-a[k]for k in a};assert delta['oom']==delta['oom_kill']==0
control_counts={x['fixture']:dict(collections.Counter(z['status']for z in x['results']['results']))for x in controls}
def raw(p):
 p=Path(p);q=p.read_bytes();return {'path':str(p),'bytes':len(q),'sha256':hashlib.sha256(q).hexdigest()}
paths=[p for p in sorted(base.iterdir())if p.is_file()]
out={'scope':'Complete unchanged pinned parallel inventory after genuine native compression backends','source_commit':r['source_commit'],'node_release':r['release'],'fixture_commit':r['fixture_commit'],'counts':counts,'total':4727,'inventory_hash':d['inventory_hash'],'jobs':4,'timeout_seconds':30,'baseline_passes':1799,'gained':c['gained'],'lost':c['lost'],'new_skips':c['new_skips'],'removed_skips':c['removed_skips'],'isolated_lost_controls':control_counts,'memory_events_delta':delta,'elapsed_seconds':r['elapsed_seconds'],'raw':{p.name:raw(p)for p in paths},'limitations':['Goal remains open:1809/4727 passes,2159 failures,82 timeouts,677 skips.','12 gained and2 lost passes; both lost cases timed out in the complete run. Isolated controls are separate observations and do not replace full-run outcomes.','Compilation overlapped this run; memory max events increased by54032 with zero OOM or OOM kills. No speed or isolated source attribution claim.','No new skips. test-icu-env.js changed skip→failure after truthful ICU metadata; this is not a pass.','Frozen c4ad source excludes later source6a error/version fixes and current Buffer allocation/pool work. Selected recoveries are not added to this full total.','All625 source hashes and all3 binaries unchanged before/after; protected permission fixture restored and bytechecked. Original jobs4 and deadline30.']}
Path('evidence/task20-node-pinned-native-codecs-full-2026-10-16.json').write_text(json.dumps(out,indent=2)+'\n');print(counts,control_counts,delta)
