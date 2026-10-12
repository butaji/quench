import hashlib, importlib.util, json, subprocess
from pathlib import Path
ROOT=Path('/workspace/quench'); OUT=ROOT/'work/task61-earley-instanceof-fastlookup-linux-2026-10-10'
spec=importlib.util.spec_from_file_location('benchrunner',ROOT/'work/task61-direct-number-screen/runner.py')
mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
raw=json.loads((ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json').read_text())
env=next(item['environment'] for item in raw['engines'] if item['name']=='quench')
base=OUT/'baseline'; candidate=OUT/'candidate'; fixture=mod.materialize('earley-boyer'); rows=[]
for pair in range(1,4):
 order=['baseline','candidate'] if pair%2 else ['candidate','baseline']; result={}
 for variant in order:
  result[variant]=mod.run(base if variant=='baseline' else candidate,fixture,env,f'instanceof-fastlookup-screen-{pair:02d}-{variant}')
 b,c=result['baseline'],result['candidate']; equal=b['semantic_output']==c['semantic_output']
 row={'pair':pair,'order':order,'valid':b['exit_code']==c['exit_code']==0 and b['score'] is not None and c['score'] is not None and equal,'output_equal':equal,'baseline_score':b['score'],'candidate_score':c['score'],'score_delta':c['score']-b['score'],'baseline_rss_bytes':b['maximum_rss_bytes'],'candidate_rss_bytes':c['maximum_rss_bytes'],'rss_delta_bytes':c['maximum_rss_bytes']-b['maximum_rss_bytes'],'baseline_wall_ns':b['wall_ns'],'candidate_wall_ns':c['wall_ns']}
 rows.append(row); print(json.dumps(row),flush=True)
(OUT/'screen-3.json').write_text(json.dumps({'experiment':'ordinary-function builtin @@hasInstance direct property-path fast path','fixture_sha256':hashlib.sha256(fixture.read_bytes()).hexdigest(),'baseline_sha256':hashlib.sha256(base.read_bytes()).hexdigest(),'candidate_sha256':hashlib.sha256(candidate.read_bytes()).hexdigest(),'source_revision':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'pairs':rows},indent=2)+'\n')
