import subprocess,os,re,sys,statistics as st,json
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; R='/home/user/quench/target/release/rss-exec'
A,B=sys.argv[1],sys.argv[2]; runs=int(sys.argv[3]); out=sys.argv[4]
FIX=['richards','deltablue','crypto','raytrace','earley-boyer','regexp','splay','navier-stokes']
modes=[('memory-first',['--jit']),('throughput',['--jit']),('memory-first',[])]
res={}
for r in range(runs):
  for pol,m in modes:
    for fx in FIX:
      for name,b in [('A',A),('B',B)]:
        e=dict(os.environ,QUENCH_COMPILE_CACHE='',QUENCH_MEMORY_POLICY=pol)
        o=subprocess.run([R,'--evict',b,'--',b,*m,f'{S}/mat/{fx}.js'],env=e,capture_output=True,text=True)
        t=o.stdout+o.stderr; sc=re.search(r'Score: (\S+)',t); rs=re.search(r'maxrss_kib=(\d+)',t)
        res.setdefault(f'{pol}{"".join(m)}|{fx}|{name}',[]).append((float(sc.group(1)) if sc else 0,int(rs.group(1)) if rs else 0))
json.dump(res,open(out,'w'))
for pol,m in modes:
  k=f'{pol}{"".join(m)}'; print('==',k)
  for fx in FIX:
    a=res[f'{k}|{fx}|A']; b=res[f'{k}|{fx}|B']
    print(f"{fx:14} rss A {min(x[1] for x in a)}-{max(x[1] for x in a)}  B {min(x[1] for x in b)}-{max(x[1] for x in b)}   score med A {st.median(x[0] for x in a):.0f} B {st.median(x[0] for x in b):.0f}")
