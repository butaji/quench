import hashlib
import json
import os
import pathlib
import re
import subprocess
import time

root = pathlib.Path('/workspace/quench')
work = root / 'target/stageb-candidates/compact-cells'
suite = root / 'quench-bench/js-engine-benchmark/v8-v7'
binaries = {'baseline': work/'baseline-quench-node', 'candidate': work/'candidate-boxed-slice-quench-node'}
fixtures = ['crypto','deltablue','earley-boyer','navier-stokes','raytrace','regexp','richards','splay']
env = {key:value for key,value in os.environ.items() if key in {'PATH','HOME','TMPDIR','LANG','LC_ALL','TZ'}}
main_rs=(root/'quench-bench/src/main.rs').read_text()
runner=re.search(r'const RUNNER: &str = r#"(.*?)"#;', main_rs, re.S).group(1)
def sha(data): return hashlib.sha256(data).hexdigest()
def semantic(text): return '\n'.join(line for line in text.splitlines() if not line.startswith(('Score: ','__quenchBenchResult: ')) and line!='----')
def run(label, fixture_path, name):
    out=work/f'boxed-slice-{name}-{label}.stdout'; err=work/f'boxed-slice-{name}-{label}.stderr'
    started=time.perf_counter_ns(); pid=os.fork()
    if pid==0:
        with out.open('wb') as o, err.open('wb') as e:
            os.dup2(o.fileno(),1); os.dup2(e.fileno(),2)
            os.execve(str(binaries[label]),[str(binaries[label]),str(fixture_path)],env)
    waited,status,usage=os.wait4(pid,0); assert waited==pid
    stdout=out.read_text(errors='replace'); scores=[line.removeprefix('Score: ') for line in stdout.splitlines() if line.startswith('Score: ')]
    return {'status':os.waitstatus_to_exitcode(status),'score':float(scores[-1]) if len(scores)==1 else None,'elapsed_ns':time.perf_counter_ns()-started,'max_rss_bytes':usage.ru_maxrss*1024,'stdout_sha256':sha(out.read_bytes()),'stderr_sha256':sha(err.read_bytes()),'semantic_sha256':sha(semantic(stdout).encode()),'semantic':semantic(stdout)}
rows=[]
for i,name in enumerate(fixtures):
    filename=suite/f'{name}.js'; source=(suite/'base.js').read_bytes()+b'\n'+filename.read_bytes()+b'\n'+runner.encode(); materialized=work/f'boxed-slice-{name}-materialized.js'; materialized.write_bytes(source)
    order=('baseline','candidate') if i%2==0 else ('candidate','baseline')
    results={label:run(label,materialized,name) for label in order}
    assert all(x['status']==0 and x['score'] is not None for x in results.values()),(name,results)
    assert results['baseline']['semantic']==results['candidate']['semantic'],name
    row={'fixture':name,'fixture_sha256':sha(source),'order':order,'baseline':{k:v for k,v in results['baseline'].items() if k!='semantic'},'candidate':{k:v for k,v in results['candidate'].items() if k!='semantic'}}
    rows.append(row)
    print(f"{name}: score_delta={row['candidate']['score']-row['baseline']['score']} rss_delta={row['candidate']['max_rss_bytes']-row['baseline']['max_rss_bytes']}",flush=True)
report={'schema':1,'experiment':'boxed-slice compact cells all-eight Quench-only one-round screen','binary_sha256':{k:sha(v.read_bytes()) for k,v in binaries.items()},'fixtures':rows,'qualification_ready':False}
(work/'boxed-slice-all-eight-quench-one-round.json').write_text(json.dumps(report,indent=2)+'\n')
