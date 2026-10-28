import json,sys,statistics as st
files=sys.argv[1:]
def load(f): return json.load(open(f))['fixtures']
pols={}
for f in files:
    pol='MF' if 'memory-first' in f else 'TP'
    pols.setdefault(pol,[]).append(load(f))
fx=list(next(iter(pols.values()))[0].keys())
print(f"{'fixture':14} {'pol':3} {'Qscore':>7} {'xNodeJL':>7} {'xBunNJ':>7} {'Qworst':>7} {'best':>7} {'RSSratio':>8}")
for fx_ in fx:
  for pol,boards in pols.items():
    qs=[];nj=[];bn=[];qw=[];best=[]
    for b in boards:
        rows={r['engine']:r for r in b[fx_]['rows']}
        q=[s['score'] for s in rows['quench']['samples']]
        n=[s['score'] for s in rows['node --jitless']['samples']]
        u=[s['score'] for s in rows['bun no-JIT']['samples']]
        # paired ratio per round
        nj+= [a/b_ for a,b_ in zip(q,n)]; bn+=[a/b_ for a,b_ in zip(q,u)]; qs+=q
        qw.append(rows['quench']['max'])
        best.append(min(r['min'] for e,r in rows.items() if not e.startswith('quench')))
    w=max(qw); bb=min(best)
    print(f"{fx_:14} {pol:3} {st.median(qs):7.0f} {st.median(nj):7.2f} {st.median(bn):7.2f} {w:7} {bb:7} {w/(0.8*bb):8.2f}")
