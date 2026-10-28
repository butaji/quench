import subprocess,os,re,sys,statistics as st
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; B=sys.argv[1]; runs=int(sys.argv[2]); ivs=sys.argv[3].split(','); fx=sys.argv[4].split(',')
for f in fx:
  for m in ([],['--jit']):
    res={}
    for r in range(runs):
      order=ivs if r%2==0 else ivs[::-1]
      for iv in order:
        e=dict(os.environ,**({'QUENCH_FIRST_CALL_DOUBLING':'1'} if iv=='dbl' else {'QUENCH_FIRST_CALL_INTERVAL_MS':iv}),QUENCH_COMPILE_CACHE=S+'/cc',QUENCH_MEMORY_POLICY='memory-first')
        o=subprocess.run([S+'/rss-exec','--evict',B,'--',B,*m,f'{S}/mat/{f}.js'],env=e,capture_output=True,text=True)
        t=o.stdout+o.stderr; rs=int(re.search(r'maxrss_kib=(\d+)',t).group(1)); sc=re.search(r'Score: (\S+)',t)
        res.setdefault(iv,[]).append((rs,float(sc.group(1)) if sc else 0))
    print(f"{f:13}{''.join(m):6}"+'  '.join(f"[{iv}] rss {st.median(x[0] for x in res[iv]):.0f} sc {st.median(x[1] for x in res[iv]):.0f}" for iv in ivs),flush=True)
