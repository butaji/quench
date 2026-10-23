import subprocess,os,re,sys,statistics as st,json
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; B=sys.argv[1]; runs=int(sys.argv[2]); configs=json.loads(sys.argv[3]); fx=sys.argv[4].split(',')
for f in fx:
  for m in ([],['--jit']):
    res={}
    for r in range(runs):
      names=list(configs) if r%2==0 else list(configs)[::-1]
      for name in names:
        e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first',**configs[name])
        o=subprocess.run([S+'/rss-exec','--evict',B,'--',B,*m,f'{S}/mat/{f}.js'],env=e,capture_output=True,text=True)
        t=o.stdout+o.stderr; rs=int(re.search(r'maxrss_kib=(\d+)',t).group(1)); sc=re.search(r'Score: (\S+)',t)
        res.setdefault(name,[]).append((rs,float(sc.group(1)) if sc else 0))
    print(f"{f:13}{''.join(m):6}"+'  '.join(f"[{n}] {st.median(x[0] for x in res[n]):.0f}/{st.median(x[1] for x in res[n]):.0f}" for n in configs),flush=True)
