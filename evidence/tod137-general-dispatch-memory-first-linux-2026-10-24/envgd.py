import subprocess,os,re,sys,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'; B=sys.argv[1]; runs=int(sys.argv[2])
ARMS={'gated':({}, 'cc-a'),'tier_on':({'QX_GD_ON':'1'},'cc-a')}
for f in sys.argv[3].split(','):
  for m in (['--jit'],):
    res={k:[] for k in ARMS}; sc={k:[] for k in ARMS}
    for r in range(runs+1):
      order=list(ARMS) if r%2==0 else list(ARMS)[::-1]
      for k in order:
        extra,cc=ARMS[k]
        e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first',QUENCH_COMPILE_CACHE=f'{S}/{cc}',**extra)
        o=subprocess.run([R,'--evict',B,'--',B,*m,f'{S}/v8fx/{f}.js'],env=e,capture_output=True,text=True)
        if r>0:
          res[k].append(int(re.search(r'maxrss_kib=(\d+)',o.stdout+o.stderr).group(1)))
          sc[k].append(int(re.search(r'Score: (\d+)',o.stdout).group(1)))
    b=st.median(res['gated']); bs=st.median(sc['gated'])
    print(f"{f:14} gated {b:.0f}/{bs:.0f}  "+'  '.join(f"{k} {st.median(res[k])-b:+.0f} ({st.median(sc[k])/bs-1:+.1%})" for k in ARMS if k!='gated'),flush=True)
