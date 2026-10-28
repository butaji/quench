import sys,subprocess,time
p=subprocess.Popen(sys.argv[1:],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
t0=time.time(); pts=[]
while p.poll() is None:
    try:
        d={}
        for l in open(f'/proc/{p.pid}/status'):
            if l.startswith(('RssAnon','RssFile','VmRSS')): d[l.split(':')[0]]=int(l.split()[1])
        if 'VmRSS' in d: pts.append((time.time()-t0,d))
    except Exception: pass
    time.sleep(0.0005)
pk=max(pts,key=lambda x:x[1]['VmRSS'])
print(f"peak {pk[1]} at {pk[0]*1000:.1f}ms; end {pts[-1][1]}")
for t,d in pts:
    if t<0.15: print(f"  {t*1000:6.1f}ms anon={d['RssAnon']:5} file={d['RssFile']:5} total={d['VmRSS']}")
