import subprocess,os,re,sys,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'; B=sys.argv[1]; runs=int(sys.argv[2])
Y=[int(v) for v in os.environ.get("YS","8192,4096,2048,1024").split(",")]
for f in ['richards','deltablue','crypto','raytrace','regexp','earley-boyer','splay','navier-stokes']:
  for m in ([],['--jit']):
    res={y:[] for y in Y}
    for r in range(runs):
      for y in (Y if r%2==0 else Y[::-1]):
        e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first',QUENCH_GC_MIN_YOUNG=str(y))
        o=subprocess.run([R,'--evict',B,'--',B,*m,f'{S}/v8fx/{f}.js'],env=e,capture_output=True,text=True)
        t=o.stdout+o.stderr; rs=int(re.search(r'maxrss_kib=(\d+)',t).group(1)); sc=re.search(r'Score: (\S+)',t)
        res[y].append((rs,float(sc.group(1)) if sc else 0))
    print(f"{f:14}{''.join(m):6}"+"  ".join(f"y{y}: rss {st.median(x[0] for x in res[y]):.0f}/{max(x[0] for x in res[y])} sc {st.median(x[1] for x in res[y]):.0f}" for y in Y),flush=True)
