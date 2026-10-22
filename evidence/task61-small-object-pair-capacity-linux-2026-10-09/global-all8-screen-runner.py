#!/usr/bin/env python3
import hashlib,json,os,re,signal,subprocess,time
from pathlib import Path
ROOT=Path('/workspace/quench'); OUT=Path(__file__).resolve().parent
FIXTURES=['crypto','deltablue','earley-boyer','navier-stokes','raytrace','regexp','richards','splay']
def sha(x): return hashlib.sha256(x).hexdigest()
def materialize(name):
 suite=ROOT/'quench-bench/js-engine-benchmark/v8-v7'; main=(ROOT/'quench-bench/src/main.rs').read_text(); m=re.search(r'const RUNNER: &str = r#"(.*?)"#;',main,re.S)
 if not m: raise RuntimeError('pinned runner missing')
 data=(suite/'base.js').read_bytes()+b'\n'+(suite/f'{name}.js').read_bytes()+b'\n'+m.group(1).encode(); p=OUT/f'materialized-{name}.js'; p.write_bytes(data); return p

def run(binary,input_path,env,label):
 d=OUT/'all8-samples'; d.mkdir(exist_ok=True); op=d/f'{label}.stdout'; ep=d/f'{label}.stderr'; of=os.open(op,os.O_CREAT|os.O_TRUNC|os.O_WRONLY,0o600); ef=os.open(ep,os.O_CREAT|os.O_TRUNC|os.O_WRONLY,0o600)
 start=time.monotonic_ns(); pid=os.fork()
 if pid==0:
  try:
   os.setsid(); os.dup2(of,1); os.dup2(ef,2); os.close(of); os.close(ef); os.execve(str(binary),[str(binary),str(input_path)],env)
  except BaseException as e: os.write(2,f'exec failed: {e}\n'.encode()); os._exit(127)
 os.close(of); os.close(ef); deadline=time.monotonic()+300
 while 1:
  waited,status,usage=os.wait4(pid,os.WNOHANG)
  if waited==pid: break
  if time.monotonic()>=deadline:
   try: os.killpg(pid,signal.SIGTERM)
   except ProcessLookupError: pass
   _,status,usage=os.wait4(pid,0); break
  time.sleep(.01)
 stdout=op.read_text(errors='replace'); stderr=ep.read_text(errors='replace'); scores=re.findall(r'^Score: ([+-]?[0-9]+(?:\.[0-9]+)?)$',stdout,re.M)
 semantic=[x for x in stdout.splitlines() if not x.startswith('Score: ') and x!='----' and not x.startswith('__quenchBenchResult: ')]
 return {'exit_code':os.waitstatus_to_exitcode(status),'score':float(scores[0]) if len(scores)==1 else None,'maximum_rss_bytes':int(usage.ru_maxrss)*1024,'wall_ns':time.monotonic_ns()-start,'semantic_output':semantic,'stdout':stdout,'stderr':stderr}

def main():
 base=(OUT/'quench-baseline').resolve(); cand=(OUT/'quench-candidate').resolve(); raw=json.load(open(ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json')); env=next(e['environment'] for e in raw['engines'] if e['name']=='quench')
 records={}; lines=[]
 for i,name in enumerate(FIXTURES):
  p=materialize(name); order=['baseline','candidate'] if i%2==0 else ['candidate','baseline']; results={}
  for variant in order: results[variant]=run(base if variant=='baseline' else cand,p,env,f'{name}-{variant}')
  b,c=results['baseline'],results['candidate']; equal=b['semantic_output']==c['semantic_output']
  records[name]={'materialized_sha256':sha(p.read_bytes()),'execution_order':order,'valid':all(x['exit_code']==0 and x['score'] is not None for x in results.values()) and equal,'output_equal':equal,'baseline_score':b['score'],'candidate_score':c['score'],'baseline_maximum_rss_bytes':b['maximum_rss_bytes'],'candidate_maximum_rss_bytes':c['maximum_rss_bytes'],'baseline_wall_ns':b['wall_ns'],'candidate_wall_ns':c['wall_ns'],'results':results}
  lines.append({'fixture':name,**records[name]})
  print(f"{name}: score {b['score']}->{c['score']}; rss {b['maximum_rss_bytes']}->{c['maximum_rss_bytes']}; output_equal={equal}",flush=True)
  with (OUT/'all8-screen.jsonl').open('a') as f: f.write(json.dumps(lines[-1],separators=(',',':'))+'\n'); f.flush(); os.fsync(f.fileno())
 report={'schema':1,'task':'61','experiment':'two-slot minimum capacity; one paired sample across all eight V8-v7 fixtures','source_revision':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'binary_sha256':{'baseline':sha(base.read_bytes()),'candidate':sha(cand.read_bytes())},'corpus_revision':raw['corpus']['pinned_revision'],'qualification_ready':False,'fixtures':records,'all_valid':all(x['valid'] for x in records.values()),'overall_decision_limit':'Single paired sample per fixture is diagnostic only; it cannot establish a Score interval or Task 61 qualification.'}
 (OUT/'all8-screen.json').write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__': main()
