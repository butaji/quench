import subprocess,os,re,sys,statistics as st
S='/tmp/claude-0/-home-user-quench/7b48bd7e-c8b8-513f-8fda-df3008a36a8f/scratchpad'; R=S+'/rss-exec'
A,B=sys.argv[1],sys.argv[2]; runs=int(sys.argv[3]); files=sys.argv[4:]
for f in files:
  for m in ([['--jit']] if os.environ.get('JITONLY') else [[]] if os.environ.get('NOJIT') else [[],['--jit']]):
    out={}
    for r in range(runs):
      for n,b in ((('A',A),('B',B)) if r%2==0 else (('B',B),('A',A))):
        e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first')
        o=subprocess.run([R,'--evict',b,'--',b,*m,f],env=e,capture_output=True,text=True)
        t=o.stdout+o.stderr; rs=re.search(r'maxrss_kib=(\d+)',t); sc=re.search(r'Score: (\S+)',t)
        out.setdefault(n,[]).append((int(rs.group(1)),float(sc.group(1)) if sc else 0))
    a,b=out['A'],out['B']
    print(f"{os.path.basename(f):16}{''.join(m):6} rss med A {st.median(x[0] for x in a):.0f} B {st.median(x[0] for x in b):.0f} d {st.median(x[0] for x in b)-st.median(x[0] for x in a):+.0f}  score A {st.median(x[1] for x in a):.0f} B {st.median(x[1] for x in b):.0f}",flush=True)
