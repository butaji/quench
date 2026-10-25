import json,statistics,sys
tag=sys.argv[1]
STRUCT={'jiti','undici'}
for mode in 'nj':
  agg={}
  for i in (1,2,3):
    for r in json.load(open(f'{tag}{mode}{i}.json'))['records']:
      if r['verdict'].endswith('failed'): continue
      n=r['package']['name']; v=agg.setdefault(n,{'kind':r['verdict']})
      for e in ('quench','bun','node'):
        x=r.get(e) or {}
        v.setdefault(e,[]).append((x.get('wall_nanos') or 0,x.get('cpu_nanos') or 0,x.get('peak_rss_bytes') or 0))
  med=lambda l,k: statistics.median(t[k] for t in l)
  rows=[]
  for n,v in agg.items():
    q,b,o=v['quench'],v['bun'],v['node']
    for k,label in ((0,'wall'),(2,'rss')):
      best=min(med(b,k),med(o,k)); qq=med(q,k)
      if qq>best:
        ratio=qq/best
        if n in STRUCT: cat='structural-builtin'
        elif v['kind']=='declarations-pass': cat='declarations-child'
        elif label=='wall' and med(q,1)<=med(b,1) and med(b,1)>med(b,0)*1.1: cat='bun-threads(cpu<=bun)'
        elif ratio<1.03: cat='noise(<3%)'
        else: cat='single-thread'
        rows.append((label,cat,n,ratio,qq/1e6 if k==0 else qq/2**20,best/1e6 if k==0 else best/2**20))
  print('MODE',mode,'losses',len(rows))
  for cat in sorted({r[1] for r in rows}):
    rs=[r for r in rows if r[1]==cat]
    print(f'  {cat}: {len(rs)}  '+', '.join(f"{r[0]}:{r[2]} {r[3]:.2f}x({r[4]:.0f}/{r[5]:.0f})" for r in sorted(rs,key=lambda r:-r[3])))
