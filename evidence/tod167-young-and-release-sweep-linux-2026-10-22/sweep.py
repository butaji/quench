# Same-binary sweep: policy x young minimum x root skipping; score and max RSS per V8-v7 fixture.
import subprocess,os,re,sys,json,math,itertools,statistics as st
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'
B=S+'/'+os.environ.get('SWEEP_BIN','qsweep')+'/quench-node'; R='/home/user/quench/target/release/rss-exec'
FIX=['richards','deltablue','crypto','raytrace','earley-boyer','regexp','splay','navier-stokes']
OLD={'QUENCH_MEASURE_TRACE_STABLE_ROOTS':'1','QUENCH_MEASURE_TRACE_ALL_FRAMES':'1'}
configs=[]
if os.environ.get('SWEEP')=='confirm':
    configs.append(('throughput/old',{'QUENCH_MEMORY_POLICY':'throughput'},'old'))
    for my in [768,4096,8192]:
        configs.append((f'mf-y{my}',{'QUENCH_MEMORY_POLICY':'memory-first','QUENCH_GC_MIN_YOUNG':str(my)},'new'))
elif os.environ.get('SWEEP')=='release':
    configs.append(('throughput/old',{'QUENCH_MEMORY_POLICY':'throughput'},'old'))
    for my in [768,8192]:
        for rel in ['both','heap','code','none']:
            configs.append((f'mf-y{my}-{rel}',{'QUENCH_MEMORY_POLICY':'memory-first','QUENCH_GC_MIN_YOUNG':str(my),'QUENCH_MEASURE_COLLECTION_RELEASE':rel},'new'))
else:
  for roots in ['old','new']:
    configs.append((f'throughput/{roots}',{'QUENCH_MEMORY_POLICY':'throughput'},roots))
    for my in [768,2048,4096,8192,16384]:
        configs.append((f'mf-y{my}/{roots}',{'QUENCH_MEMORY_POLICY':'memory-first','QUENCH_GC_MIN_YOUNG':str(my)},roots))
runs=int(sys.argv[1]) if len(sys.argv)>1 else 2
mode=sys.argv[2:] if len(sys.argv)>2 else []
res={}
for r in range(runs):
    for name,env,roots in configs:
        e=dict(os.environ); e['QUENCH_COMPILE_CACHE']=''; e.update(env)
        if roots=='old': e.update(OLD)
        for fx in FIX:
            out=subprocess.run([R,'--evict',B,'--',B,*mode,f'{S}/mat/{fx}.js'],env=e,capture_output=True,text=True)
            t=out.stdout+out.stderr
            sc=re.search(r'Score: (\S+)',t); rss=re.search(r'maxrss_kib=(\d+)',t)
            res.setdefault((name,fx),[]).append((float(sc.group(1)) if sc else float('nan'),int(rss.group(1)) if rss else 0))
json.dump({f'{k[0]}|{k[1]}':v for k,v in res.items()},open(S+f'/sweep-{os.environ.get("SWEEP","young")}{"-jit" if mode else ""}.json','w'))
base='throughput/old'
print(f"{'config':20}{'score_geo':>10}{'rss_geo':>9}  per-fixture maxRSS KiB")
for name,_,_ in configs:
    sr=[];rr=[];row=[]
    for fx in FIX:
        s=st.median(x[0] for x in res[(name,fx)]); m=max(x[1] for x in res[(name,fx)])
        sb=st.median(x[0] for x in res[(base,fx)]); mb=max(x[1] for x in res[(base,fx)])
        sr.append(s/sb); rr.append(m/mb); row.append(m)
    g=lambda v: math.exp(sum(map(math.log,v))/len(v))
    print(f"{name:20}{g(sr):10.3f}{g(rr):9.3f}  "+' '.join(f'{x:6}' for x in row))
