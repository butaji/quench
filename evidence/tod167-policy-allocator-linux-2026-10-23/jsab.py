import subprocess,os,re,sys,statistics as st
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; R='/home/user/quench/quench-bench/jetstream'
A,B=sys.argv[1],sys.argv[2]; runs=int(sys.argv[3]); pol=sys.argv[4]; tests=sys.argv[5:]
for t in tests:
  res={}
  for r in range(runs):
    for n,b in ((('A',A),('B',B)) if r%2==0 else (('B',B),('A',A))):
      e=dict(os.environ,QUENCH_MEMORY_POLICY=pol)
      o=subprocess.run([S+'/rss-exec','--evict',b,'--',b,S+'/js-prelude.js',R,'--no-prefetch',t],env=e,capture_output=True,text=True,timeout=900)
      txt=o.stdout+o.stderr; sc=re.search(r'Overall Score\s+([\d.]+)',txt); rs=re.search(r'maxrss_kib=(\d+)',txt)
      res.setdefault(n,[]).append((float(sc.group(1)) if sc else 0,int(rs.group(1)) if rs else 0))
  a,b=res['A'],res['B']; sa,sb=st.median(x[0] for x in a),st.median(x[0] for x in b)
  print(f"{t:24} {pol:12} score A {sa:7.2f} B {sb:7.2f} x{sb/sa if sa else 0:.3f}  rss A {st.median(x[1] for x in a):.0f} B {st.median(x[1] for x in b):.0f}",flush=True)
