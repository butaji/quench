#!/usr/bin/env python3
# pair.py BASE CAND N bench... : interleaved stock pairs, alternating order; prints medians and paired score ratio
import subprocess,sys,statistics as st,os,re
S=os.path.dirname(os.path.abspath(__file__))
base,cand,n=sys.argv[1],sys.argv[2],int(sys.argv[3]); benches=sys.argv[4:]
of=f'{S}/pair.{os.getpid()}'
def run(binary,b):
    r=subprocess.run([f'{S}/spawn',of,binary,f'{S}/mat/{b}.js'],capture_output=True,text=True).stdout.split()
    t=open(of).read(); m=re.search(r'^Score: (\S+)',t,re.M)
    return (float(m.group(1)) if m else float('nan')), int(r[0])/1024
for b in benches:
    rows=[]
    for i in range(n):
        order=[(0,base),(1,cand)] if i%2==0 else [(1,cand),(0,base)]
        res={}
        for k,binary in order: res[k]=run(binary,b)
        rows.append(res)
    bs=[r[0][0] for r in rows]; cs=[r[1][0] for r in rows]; br=[r[0][1] for r in rows]; cr=[r[1][1] for r in rows]
    ratios=sorted(c/x for c,x in zip(cs,bs)); wins=sum(c>x for c,x in zip(cs,bs))
    print(f"{b:14} score {st.median(bs):7.0f} -> {st.median(cs):7.0f}  paired ratio median {st.median(ratios):.3f} [{ratios[0]:.3f}..{ratios[-1]:.3f}] cand ahead {wins}/{n}   rss {st.median(br):6.1f} -> {st.median(cr):6.1f} MB",flush=True)
