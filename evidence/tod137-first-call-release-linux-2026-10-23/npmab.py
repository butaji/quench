import subprocess,os,re,sys,time,statistics as st
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; B=sys.argv[1]; runs=int(sys.argv[2]); ivs=sys.argv[3].split(','); files=sys.argv[4:]
pol=os.environ.get('POL','memory-first')
for f in files:
  res={}
  for r in range(runs):
    for iv in (ivs if r%2==0 else ivs[::-1]):
      e=dict(os.environ,**({'QUENCH_FIRST_CALL_DOUBLING':'1'} if iv=='dbl' else {'QUENCH_FIRST_CALL_INTERVAL_MS':iv}),QUENCH_COMPILE_CACHE=S+'/cc',QUENCH_MEMORY_POLICY=pol)
      t0=time.perf_counter(); o=subprocess.run([S+'/rss-exec','--evict',B,'--',B,f],env=e,capture_output=True,text=True); dt=time.perf_counter()-t0
      rs=int(re.search(r'maxrss_kib=(\d+)',o.stderr).group(1))
      res.setdefault(iv,[]).append((rs,dt*1000))
  print(f"{os.path.basename(f):10} {pol:12}"+'  '.join(f"[{iv}] rss {st.median(x[0] for x in res[iv]):.0f} ms {st.median(x[1] for x in res[iv]):.0f}" for iv in ivs),flush=True)
