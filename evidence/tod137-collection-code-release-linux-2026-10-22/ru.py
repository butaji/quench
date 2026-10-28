import resource,subprocess,sys,os,re,statistics as st
binp=sys.argv[1]; fx=sys.argv[2]; n=int(sys.argv[3]); extra=sys.argv[4:]
for P in ['throughput','memory-first']:
  rows=[]
  for i in range(n):
    e=dict(os.environ,QUENCH_MEMORY_POLICY=P)
    b=resource.getrusage(resource.RUSAGE_CHILDREN)
    o=subprocess.run([binp,*extra,fx],env=e,capture_output=True,text=True).stdout
    a=resource.getrusage(resource.RUSAGE_CHILDREN)
    sc=float(re.search(r'Score: (\S+)',o).group(1))
    rows.append((sc,a.ru_utime-b.ru_utime,a.ru_stime-b.ru_stime,a.ru_minflt-b.ru_minflt))
  m=lambda k: st.median(r[k] for r in rows)
  print(f"{P:13} score {m(0):7.0f} user {m(1):5.2f}s sys {m(2):5.2f}s minflt {m(3):8.0f}")
