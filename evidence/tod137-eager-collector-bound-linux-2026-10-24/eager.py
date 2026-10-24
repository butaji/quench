import subprocess,os,re,sys,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'; B=sys.argv[1]; runs=int(sys.argv[2])
EAGER=dict(QUENCH_GC_MIN_YOUNG='64',QUENCH_GC_MAX_YOUNG='64',QUENCH_GC_YOUNG_NUM='1',QUENCH_GC_YOUNG_DEN='4096',
  QUENCH_GC_OLD_GROWTH_NUM='1',QUENCH_GC_OLD_GROWTH_DEN='64',QUENCH_GC_MIN_OLD_GROWTH='64',
  QUENCH_GC_ALLOCATION_CAP_NUM='1',QUENCH_GC_ALLOCATION_CAP_DEN='64',QUENCH_GC_MIN_ALLOCATION_CAP='64')
ARMS={'default':{},'eager':EAGER}
for f in sys.argv[3].split(','):
  for m in ([],['--jit']):
    res={k:[] for k in ARMS}
    for r in range(runs+1):
      for k in (list(ARMS) if r%2==0 else list(ARMS)[::-1]):
        e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first',QUENCH_COMPILE_CACHE=f'{S}/cc-e',**ARMS[k])
        o=subprocess.run([R,'--evict',B,'--',B,*m,f'{S}/v8fx/{f}.js'],env=e,capture_output=True,text=True)
        t=o.stdout+o.stderr
        if r>0: res[k].append((int(re.search(r'maxrss_kib=(\d+)',t).group(1)),int(re.search(r'Score: (\d+)',t).group(1))))
    d=res['default'];g=res['eager']
    print(f"{f:14}{''.join(m):6} default med {st.median(x[0] for x in d):.0f} min {min(x[0] for x in d)}  eager med {st.median(x[0] for x in g):.0f} min {min(x[0] for x in g)}  d {st.median(x[0] for x in g)-st.median(x[0] for x in d):+.0f}  score {st.median(x[1] for x in d):.0f}->{st.median(x[1] for x in g):.0f}",flush=True)
