import subprocess,os,re,sys,statistics as st,resource
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'; B=S+'/qgw/quench-node'; runs=int(sys.argv[1])
ARMS={'old':{'QX_OLD':'1'},'grow':{}}
def run(cmd,env,cwd=None):
    e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first',**env)
    r0=resource.getrusage(resource.RUSAGE_CHILDREN)
    o=subprocess.run([R,'--evict',B,'--',B,*cmd],env=e,capture_output=True,text=True,cwd=cwd); t=o.stdout+o.stderr
    r1=resource.getrusage(resource.RUSAGE_CHILDREN)
    sc=re.search(r'Score: (\S+)',t)
    return int(re.search(r'maxrss_kib=(\d+)',t).group(1)), float(sc.group(1)) if sc else (r1.ru_utime-r0.ru_utime+r1.ru_stime-r0.ru_stime)*1000
items=[(f,[*m,f'{S}/v8fx/{f}.js'],None) for f in sys.argv[2].split(',') for m in ([],['--jit'])]
items+=[(p,['/home/user/quench/tests/npm-top300/scenario.mjs',p],S+'/tse') for p in sys.argv[3].split(',') if p]
for name,cmd,cwd in items:
    res={k:[] for k in ARMS}
    run(cmd,{},cwd)
    for r in range(runs):
        for k in (list(ARMS) if r%2==0 else list(ARMS)[::-1]): res[k].append(run(cmd,ARMS[k],cwd))
    print(f"{name:14}{'jit' if '--jit' in cmd else '':4} "+' | '.join(f"{k} med {st.median(x[0] for x in v):.0f} max {max(x[0] for x in v)} s {st.median(x[1] for x in v):.0f}" for k,v in res.items()),flush=True)
