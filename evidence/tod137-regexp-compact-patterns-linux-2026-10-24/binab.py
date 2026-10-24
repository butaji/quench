import subprocess,os,re,sys,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'
bins={k:f'{S}/{k}/quench-node' for k in sys.argv[2].split(',')}; runs=int(sys.argv[1]); pol=os.environ.get('POL','memory-first')
def run(b,cmd):
    e=dict(os.environ,QUENCH_MEMORY_POLICY=pol)
    t=subprocess.run([R,'--evict',b,'--',b,*cmd],env=e,capture_output=True,text=True); t=t.stdout+t.stderr
    sc=re.search(r'Score: (\S+)',t); return int(re.search(r'maxrss_kib=(\d+)',t).group(1)), float(sc.group(1)) if sc else 0
for f in sys.argv[3].split(','):
  for m in ([],['--jit']):
    cmd=[*m,f'{S}/v8fx/{f}.js']; res={k:[] for k in bins}
    for b in bins.values(): run(b,cmd)
    for r in range(runs):
        for k in (list(bins) if r%2==0 else list(bins)[::-1]): res[k].append(run(bins[k],cmd))
    print(f"{f:14}{'jit' if m else '':4} "+' | '.join(f"{k} med {st.median(x[0] for x in v):.0f} max {max(x[0] for x in v)} min {min(x[0] for x in v)} s {st.median(x[1] for x in v):.0f}" for k,v in res.items()),flush=True)
