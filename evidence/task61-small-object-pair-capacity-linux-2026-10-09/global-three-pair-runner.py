#!/usr/bin/env python3
import importlib.util,json,random,subprocess
from pathlib import Path
ROOT=Path('/workspace/quench'); OUT=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('pair_runner',OUT/'pairs.py'); mod=importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
FIXTURES=['earley-boyer.js']; N=3
raw=json.load(open(ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json')); env=next(e['environment'] for e in raw['engines'] if e['name']=='quench')
base=(OUT/'quench-baseline').resolve(); cand=(OUT/'quench-candidate').resolve(); all_records={}; log=OUT/'earley-three-pairs.jsonl'; log.unlink(missing_ok=True)
for fixture in FIXTURES:
 path,materialized_hash=mod.materialize(fixture); records=[]
 for pair in range(1,N+1):
  order=['baseline','candidate'] if pair%2 else ['candidate','baseline']; results={}
  for variant in order:
   binary=base if variant=='baseline' else cand
   results[variant]=mod.run_one(binary,path,env,f'{fixture[:-3]}-pair-{pair:02d}-{variant}')
  b,c=results['baseline'],results['candidate']; equal=b['semantic_output']==c['semantic_output']; valid=all(x['exit_code']==0 and x['score'] is not None for x in results.values()) and equal
  rec={'fixture':fixture,'pair':pair,'execution_order':order,'valid':valid,'output_equal':equal,'baseline_score':b['score'],'candidate_score':c['score'],'baseline_rss_bytes':b['maximum_rss_bytes'],'candidate_rss_bytes':c['maximum_rss_bytes'],'results':results}; records.append(rec)
  with log.open('a') as f: f.write(json.dumps(rec,separators=(',',':'))+'\n'); f.flush()
  print(f"{fixture} pair {pair}/{N}: score {b['score']}->{c['score']}; rss {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']}; output_equal={equal}",flush=True)
 all_records[fixture]={'materialized_sha256':materialized_hash,'valid':all(r['valid'] for r in records),'output_equal':all(r['output_equal'] for r in records),'records':records}
report={'schema':1,'task':'61','experiment':'three-pair control screen for two-slot ValueArena minimum','source_revision':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'corpus_revision':raw['corpus']['pinned_revision'],'rounds_per_fixture':N,'qualification_ready':False,'fixtures':all_records,'binary_sha256':{'baseline':__import__('hashlib').sha256(base.read_bytes()).hexdigest(),'candidate':__import__('hashlib').sha256(cand.read_bytes()).hexdigest()}}
(OUT/'earley-three-pairs.json').write_text(json.dumps(report,indent=2)+'\n')
