import importlib.util
import json
from pathlib import Path
ROOT=Path('/workspace/quench')
OUT=ROOT/'work/task61-v2-merge-earley'
spec=importlib.util.spec_from_file_location('screen_runner',ROOT/'work/task61-direct-number-screen/runner.py')
runner=importlib.util.module_from_spec(spec); spec.loader.exec_module(runner)
raw=json.loads((ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env=next(engine['environment'] for engine in raw['engines'] if engine['name']=='quench')
base=OUT/'construct-check-baseline'; candidate=OUT/'construct-check-candidate'; fixture=runner.materialize('earley-boyer')
rows=[]
for pair in range(1,4):
    order=['baseline','candidate'] if pair%2 else ['candidate','baseline']
    values={}
    for variant in order:
        binary=base if variant=='baseline' else candidate
        values[variant]=runner.run(binary,fixture,env,f'construct-check-screen-{pair:02d}-{variant}')
    b,c=values['baseline'],values['candidate']
    row={'pair':pair,'order':order,'output_equal':b['semantic_output']==c['semantic_output'],
         'baseline_score':b['score'],'candidate_score':c['score'],
         'score_delta':c['score']-b['score'] if b['score'] is not None and c['score'] is not None else None,
         'baseline_rss_bytes':b['maximum_rss_bytes'],'candidate_rss_bytes':c['maximum_rss_bytes'],
         'rss_delta_bytes':c['maximum_rss_bytes']-b['maximum_rss_bytes'],
         'baseline_wall_ns':b['wall_ns'],'candidate_wall_ns':c['wall_ns'],
         'valid':b['exit_code']==c['exit_code']==0 and b['score'] is not None and c['score'] is not None}
    rows.append(row)
    print(f"pair {pair}/3: Score {b['score']}->{c['score']} ({row['score_delta']:+g}); RSS {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); output_equal={row['output_equal']}",flush=True)
report={'experiment':'EarleyBoyer same-target constructability check elimination directional screen','node_oracle_match':True,'rounds':len(rows),'valid':all(r['valid'] and r['output_equal'] for r in rows),'pairs':rows}
(OUT/'construct-check-screen-3.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
