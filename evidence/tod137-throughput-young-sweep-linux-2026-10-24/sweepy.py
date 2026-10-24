import subprocess,os,re,sys,resource,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); B=S+'/qsy/quench-node'; R=S+'/rss-exec'
ARMS={'base':{},'m128':{'QUENCH_GC_MIN_YOUNG':'131072','QUENCH_GC_MAX_YOUNG':'2097152'},'m256':{'QUENCH_GC_MIN_YOUNG':'262144','QUENCH_GC_MAX_YOUNG':'2097152'},'g4':{'QUENCH_GC_SURVIVAL_GROWTH':'4'},'g8':{'QUENCH_GC_SURVIVAL_GROWTH':'8'}}
mode=sys.argv[1]; runs=int(sys.argv[2])
def run(cmd,env):
    e=dict(os.environ,QUENCH_MEMORY_POLICY='throughput',**env)
    r0=resource.getrusage(resource.RUSAGE_CHILDREN)
    o=subprocess.run([R,'--',*cmd],env=e,capture_output=True,text=True,cwd=S+'/tse')
    r1=resource.getrusage(resource.RUSAGE_CHILDREN)
    t=o.stdout+o.stderr
    return (r1.ru_utime-r0.ru_utime+r1.ru_stime-r0.ru_stime)*1000, int(re.search(r'maxrss_kib=(\d+)',t).group(1)), float(m.group(1)) if (m:=re.search(r'Score: (\S+)',t)) else 0, t
items = sys.argv[3].split(',')
for it in items:
    cmd=[B,'/home/user/quench/tests/npm-top300/scenario.mjs',it] if mode=='npm' else [B,f'{S}/v8fx/{it}.js']
    run(cmd,{})
    res={k:[] for k in ARMS}
    for r in range(runs):
        for k in (list(ARMS) if r%2==0 else list(ARMS)[::-1]):
            c,m,sc,t=run(cmd,ARMS[k])
            if mode=='npm': assert 'npm-behavior-ok' in t, t[-300:]
            res[k].append((c,m,sc))
    b=res['base']; bc=st.median(x[0] for x in b); bm=st.median(x[1] for x in b); bs=st.median(x[2] for x in b)
    def fmt(k):
        v=res[k]; c=st.median(x[0] for x in v); m=st.median(x[1] for x in v); s=st.median(x[2] for x in v)
        return f"{k} cpu {c/bc-1:+.0%} sc {(s/bs-1) if bs else 0:+.0%} rss {(m-bm)/1024:+.1f}MB"
    print(f"{it:34} base cpu {bc:.0f} sc {bs:.0f} rss {bm/1024:.1f}MB | "+' | '.join(fmt(k) for k in ARMS if k!='base'),flush=True)
