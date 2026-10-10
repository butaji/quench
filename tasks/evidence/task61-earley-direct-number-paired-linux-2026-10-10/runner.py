#!/usr/bin/env python3
import hashlib, importlib.util, json, random, statistics, subprocess
from pathlib import Path
ROOT=Path('/workspace/quench'); OUT=Path('/workspace/quench/work/task61-direct-number-screen/candidate4-add-double-multiply')
FIXTURES=['earley-boyer.js']; N=11; BOOT=100000
spec=importlib.util.spec_from_file_location('screen_runner',ROOT/'work/task61-direct-number-screen/runner.py')
mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
raw=json.load(open(ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json'))
env=next(e['environment'] for e in raw['engines'] if e['name']=='quench')
base=(ROOT/'work/task61-from-char-code-linux-2026-10-09/quench-baseline').resolve()
cand=(ROOT/'target/production/quench-node').resolve()
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def interval(values,seed):
 rng=random.Random(seed); draws=[]; size=len(values)
 for _ in range(BOOT): draws.append(statistics.median(rng.choices(values,k=size)))
 draws.sort(); return [draws[int(.025*BOOT)],draws[int(.975*BOOT)]]
report={'schema':1,'task':'61','experiment':'direct numeric dispatch paired follow-up','source_revision':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'baseline_sha256':sha(base),'candidate_sha256':sha(cand),'rounds_per_fixture':N,'bootstrap_replicates':BOOT,'corpus_revision':raw['corpus']['pinned_revision'],'fixtures':{}}
log=(OUT/'candidate4-paired-11.jsonl')
existing={}
if log.exists():
 for line in log.read_text().splitlines():
  rec=json.loads(line); existing.setdefault(rec['fixture'],[]).append(rec)
for fi,fixture in enumerate(FIXTURES):
 path=mod.materialize(fixture[:-3]); fixture_sha=sha(path); pairs=existing.get(fixture,[])
 for pair in range(len(pairs)+1,N+1):
  order=['baseline','candidate'] if pair%2 else ['candidate','baseline']; results={}
  for variant in order:
   binary=base if variant=='baseline' else cand
   results[variant]=mod.run(binary,path,env,f'candidate4-{fixture[:-3]}-pair-{pair:02d}-{variant}')
  b,c=results['baseline'],results['candidate']; equal=b['semantic_output']==c['semantic_output']
  rec={'fixture':fixture,'pair':pair,'execution_order':order,'valid':all(x['exit_code']==0 and x['score'] is not None for x in results.values()) and equal,'output_equal':equal,'baseline_score':b['score'],'candidate_score':c['score'],'score_delta':c['score']-b['score'],'baseline_rss_bytes':b['maximum_rss_bytes'],'candidate_rss_bytes':c['maximum_rss_bytes'],'rss_delta_bytes':c['maximum_rss_bytes']-b['maximum_rss_bytes'],'baseline_wall_ns':b['wall_ns'],'candidate_wall_ns':c['wall_ns'],'results':results}
  pairs.append(rec)
  with log.open('a') as f: f.write(json.dumps(rec,separators=(',',':'))+'\n'); f.flush()
  print(f"{fixture} pair {pair}/{N}: score {b['score']}->{c['score']} ({rec['score_delta']:+g}); rss {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']} ({rec['rss_delta_bytes']:+d}); equal={equal}",flush=True)
 score=[x['score_delta'] for x in pairs]; rss=[x['rss_delta_bytes'] for x in pairs]
 report['fixtures'][fixture]={'fixture_sha256':fixture_sha,'valid':all(x['valid'] for x in pairs),'output_equal':all(x['output_equal'] for x in pairs),'baseline_score_median':statistics.median(x['baseline_score'] for x in pairs),'candidate_score_median':statistics.median(x['candidate_score'] for x in pairs),'score_delta_median':statistics.median(score),'score_delta_95_ci':interval(score,0x610000+fi),'baseline_maximum_rss_median_bytes':statistics.median(x['baseline_rss_bytes'] for x in pairs),'candidate_maximum_rss_median_bytes':statistics.median(x['candidate_rss_bytes'] for x in pairs),'maximum_rss_delta_median_bytes':statistics.median(rss),'maximum_rss_delta_95_ci_bytes':interval(rss,0x620000+fi),'candidate_score_worse_pairs':sum(x<0 for x in score),'candidate_rss_lower_pairs':sum(x<0 for x in rss),'pairs':pairs}
 (OUT/'candidate4-paired-11.json').write_text(json.dumps(report,indent=2)+'\n')
report['qualification_ready']=False
report['decision_limit']='Eight-fixture Quench-only paired follow-up; this cannot establish the four-engine all-eight Task 61 gate.'
(OUT/'candidate4-paired-11.json').write_text(json.dumps(report,indent=2)+'\n')
