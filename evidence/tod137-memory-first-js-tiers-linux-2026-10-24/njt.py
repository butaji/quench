import subprocess,os,re,sys,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'; B=sys.argv[1]; runs=int(sys.argv[2])
ARMS={'jit':(['--jit'],{}),'jit_notier':(['--jit'],{'QX_NOJSTIER':'1'}),'nojit':([],{})}
for f in sys.argv[3].split(','):
    res={k:[] for k in ARMS}
    for r in range(runs+1):
        for k in (list(ARMS) if r%2==0 else list(ARMS)[::-1]):
            a,env=ARMS[k]
            e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first',**env)
            o=subprocess.run([R,'--evict',B,'--',B,*a,f'{S}/v8fx/{f}.js'],env=e,capture_output=True,text=True); t=o.stdout+o.stderr
            if r>0: res[k].append((int(re.search(r'maxrss_kib=(\d+)',t).group(1)),float(re.search(r'Score: (\S+)',t).group(1))))
    print(f"{f:14} "+' | '.join(f"{k} {st.median(x[0] for x in v):.0f} KiB sc {st.median(x[1] for x in v):.0f}" for k,v in res.items()),flush=True)
