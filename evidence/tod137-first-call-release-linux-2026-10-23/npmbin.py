import subprocess,os,re,sys,time,statistics as st
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; A,B=sys.argv[1],sys.argv[2]; runs=int(sys.argv[3]); pol=sys.argv[4]; files=sys.argv[5:]
for f in files:
  res={}
  for r in range(runs):
    for n,b in ((('A',A),('B',B)) if r%2==0 else (('B',B),('A',A))):
      e=dict(os.environ,QUENCH_COMPILE_CACHE=S+'/cc',QUENCH_MEMORY_POLICY=pol)
      t0=time.perf_counter(); o=subprocess.run([S+'/rss-exec','--evict',b,'--',b,f],env=e,capture_output=True,text=True); dt=time.perf_counter()-t0
      res.setdefault(n,[]).append((int(re.search(r'maxrss_kib=(\d+)',o.stderr).group(1)),dt*1000))
  a,b=res['A'],res['B']
  print(f"{os.path.basename(f):10} {pol:12} rss A {st.median(x[0] for x in a):.0f} B {st.median(x[0] for x in b):.0f}  ms A {st.median(x[1] for x in a):.0f} B {st.median(x[1] for x in b):.0f}",flush=True)
