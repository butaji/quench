#!/usr/bin/env python3
import hashlib, json, math, os, platform, random, re, signal, statistics, subprocess, time
from pathlib import Path
ROOT = Path('/workspace/quench')
FIXTURE = 'crypto.js'
PAIR_COUNT = 11
BOOTSTRAPS = 100_000
SEED = 20261009
TIMEOUT = 300
OUT = Path(__file__).resolve().parent

def sha(b): return hashlib.sha256(b).hexdigest()
def quantile(v, p):
    v=sorted(v); x=(len(v)-1)*p; lo=math.floor(x); hi=math.ceil(x)
    return v[lo] if lo==hi else v[lo]+(v[hi]-v[lo])*(x-lo)
def bootstrap(a,b,rng):
    diffs=[]
    for _ in range(BOOTSTRAPS):
        ix=[rng.randrange(PAIR_COUNT) for _ in range(PAIR_COUNT)]
        diffs.append(statistics.median(b[i] for i in ix)-statistics.median(a[i] for i in ix))
    return {'lower_95':quantile(diffs,.025),'upper_95':quantile(diffs,.975),'resamples':BOOTSTRAPS,'method':'paired percentile bootstrap of difference in medians','seed':SEED}
def materialize(fixture=None):
    fixture = fixture or FIXTURE
    suite=ROOT/'quench-bench/js-engine-benchmark/v8-v7'
    main=(ROOT/'quench-bench/src/main.rs').read_text()
    m=re.search(r'const RUNNER: &str = r#"(.*?)"#;',main,re.S)
    if not m: raise RuntimeError('could not extract pinned runner')
    data=(suite/'base.js').read_bytes()+b'\n'+(suite/fixture).read_bytes()+b'\n'+m.group(1).encode()
    path=OUT/f'materialized-{fixture.removesuffix(".js")}.js'; path.write_bytes(data)
    return path,sha(data)
def run_one(binary,input_path,env,label):
    stdout_path=OUT/'samples'/f'{label}.stdout'; stderr_path=OUT/'samples'/f'{label}.stderr'
    stdout_path.parent.mkdir(parents=True,exist_ok=True)
    ofd=os.open(stdout_path,os.O_CREAT|os.O_TRUNC|os.O_WRONLY,0o600); efd=os.open(stderr_path,os.O_CREAT|os.O_TRUNC|os.O_WRONLY,0o600)
    start=time.monotonic_ns(); pid=os.fork()
    if pid==0:
        try:
            os.setsid(); os.dup2(ofd,1); os.dup2(efd,2); os.close(ofd); os.close(efd); os.execve(str(binary),[str(binary),str(input_path)],env)
        except BaseException as e:
            os.write(2,f'exec failed: {e}\n'.encode()); os._exit(127)
    os.close(ofd); os.close(efd); deadline=time.monotonic()+TIMEOUT; timed=False
    while True:
        waited,status,usage=os.wait4(pid,os.WNOHANG)
        if waited==pid: break
        if time.monotonic()>=deadline:
            timed=True
            try: os.killpg(pid,signal.SIGTERM)
            except ProcessLookupError: pass
            grace=time.monotonic()+1
            while time.monotonic()<grace:
                waited,status,usage=os.wait4(pid,os.WNOHANG)
                if waited==pid: break
                time.sleep(.02)
            else:
                try: os.killpg(pid,signal.SIGKILL)
                except ProcessLookupError: pass
                _,status,usage=os.wait4(pid,0)
            break
        time.sleep(.01)
    stdout=stdout_path.read_text(errors='replace'); stderr=stderr_path.read_text(errors='replace')
    scores=[float(x) for x in re.findall(r'^Score: ([+-]?[0-9]+(?:\.[0-9]+)?)$',stdout,re.M)]
    result_lines=[x for x in stdout.splitlines() if x.startswith('__quenchBenchResult: ')]
    semantic=[x for x in stdout.splitlines() if not x.startswith('Score: ') and x!='----' and not x.startswith('__quenchBenchResult: ')]
    return {'command':[str(binary),str(input_path)],'exit_code':os.waitstatus_to_exitcode(status),'timed_out':timed,'score':scores[0] if len(scores)==1 else None,'maximum_rss_bytes':int(usage.ru_maxrss)*1024,'wall_ns':time.monotonic_ns()-start,'result_lines':result_lines,'semantic_output':semantic,'stdout':stdout,'stderr':stderr}
def main():
    base=(OUT/'quench-baseline').resolve(); cand=(OUT/'quench-candidate').resolve()
    raw=json.load(open(ROOT/'tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json'))
    env=next(e['environment'] for e in raw['engines'] if e['name']=='quench')
    input_path,input_hash=materialize(); rng=random.Random(SEED); records=[]
    log=OUT/'pairs.jsonl'; log.unlink(missing_ok=True)
    for pair in range(1,PAIR_COUNT+1):
        order=['baseline','candidate'] if pair%2 else ['candidate','baseline']; results={}
        for variant in order:
            binary=base if variant=='baseline' else cand
            results[variant]=run_one(binary,input_path,env,f'pair-{pair:02d}-{variant}')
        b,c=results['baseline'],results['candidate']
        valid=all(x['exit_code']==0 and not x['timed_out'] and x['score'] is not None and math.isfinite(x['score']) and x['maximum_rss_bytes']>0 for x in results.values())
        equal=b['semantic_output']==c['semantic_output']
        valid=valid and equal
        record={'pair':pair,'execution_order':order,'valid':valid,'output_equal':equal,'results':results}
        records.append(record)
        with log.open('a') as f: f.write(json.dumps(record,separators=(',',':'))+'\n'); f.flush(); os.fsync(f.fileno())
        print(f"Crypto pair {pair}/{PAIR_COUNT} order={order} valid={valid} score={b['score']}->{c['score']} rss={b['maximum_rss_bytes']}->{c['maximum_rss_bytes']}",flush=True)
    bs=[r['results']['baseline'] for r in records]; cs=[r['results']['candidate'] for r in records]
    bscore=[x['score'] for x in bs]; cscore=[x['score'] for x in cs]; brss=[x['maximum_rss_bytes'] for x in bs]; crss=[x['maximum_rss_bytes'] for x in cs]
    report={'schema':1,'task':'61','experiment':'single-numeric-argument String.fromCharCode fast path','fixture':FIXTURE,'source_revision':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip(),'candidate_dirty':True,'candidate_diff_sha256':sha(subprocess.check_output(['git','-C',str(ROOT),'diff','--binary'])),'host':{'uname':platform.uname()._asdict(),'rustc':subprocess.run(['/workspace/quench/work/stageb-tools/rustup-home/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc','--version','--verbose'],capture_output=True,text=True).stdout,'cpu_quota':Path('/sys/fs/cgroup/cpu.max').read_text().strip(),'memory_limit_bytes':int(Path('/sys/fs/cgroup/memory.max').read_text().strip()),'rss_backend':'Linux wait4 ru_maxrss, KiB normalized to bytes'},'corpus_revision':subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD:quench-bench/js-engine-benchmark'],text=True).strip() if False else raw['corpus']['pinned_revision'],'source_fixture_sha256':sha((ROOT/'quench-bench/js-engine-benchmark/v8-v7'/FIXTURE).read_bytes()),'materialized_sha256':input_hash,'materialized_path':str(input_path.relative_to(ROOT)),'rounds_per_fixture':PAIR_COUNT,'bootstrap_resamples':BOOTSTRAPS,'binary_sha256':{'baseline':sha(base.read_bytes()),'candidate':sha(cand.read_bytes())},'valid':all(r['valid'] for r in records),'outputs_equal':all(r['output_equal'] for r in records),'rounds':records,'summary':{'baseline_median_score':statistics.median(bscore),'candidate_median_score':statistics.median(cscore),'score_delta':statistics.median(cscore)-statistics.median(bscore),'score_delta_percent':(statistics.median(cscore)/statistics.median(bscore)-1)*100,'score_delta_95_percentile_interval':bootstrap(bscore,cscore,rng),'baseline_median_maximum_rss_bytes':statistics.median(brss),'candidate_median_maximum_rss_bytes':statistics.median(crss),'maximum_rss_delta_bytes':statistics.median(crss)-statistics.median(brss),'maximum_rss_delta_95_percentile_interval':bootstrap(brss,crss,rng),'candidate_lower_score_pairs':sum(c<b for b,c in zip(bscore,cscore)),'candidate_lower_rss_pairs':sum(c<b for b,c in zip(brss,crss))}}
    out=OUT/'pairs.json'; out.write_text(json.dumps(report,indent=2)+'\n'); print(json.dumps(report['summary'],indent=2))
if __name__=='__main__': main()
