import re,sys,collections
snaps=[];cur=None;stack=[]
for line in open(sys.argv[1]):
    if line.startswith('snapshot='): cur={'nodes':[]}; snaps.append(cur); stack=[]; continue
    m=re.match(r'^( *)n(\d+): (\d+) (.*)$',line.rstrip('\n'))
    if m and cur is not None:
        depth=len(m.group(1)); name=re.sub(r'^0x[0-9A-F]+: ','',m.group(4)); name=re.sub(r' \(.*\)$','',name)
        stack=stack[:depth]+[name]; cur['nodes'].append((tuple(stack),int(m.group(3)),int(m.group(2))))
det=[s for s in snaps if len(s['nodes'])>1]
K=int(sys.argv[2]) if len(sys.argv)>2 else 2
def owner(st):
    q=[f for f in st[1:] if 'quench' in f and 'thin_slice' not in f and 'quench_alloc' not in f]
    return ' <- '.join(re.sub(r'<[^<>]*(<[^<>]*>[^<>]*)*>','',f)[:90] for f in q[:K]) if q else st[-1][:80]
def agg(s):
    d=collections.Counter()
    for st,b,n in s['nodes']:
        if n==0: d[owner(st)]+=b
    return d
A,B=agg(det[1]),agg(det[-1])
for k in sorted(set(A)|set(B),key=lambda k:-(B[k]-A[k]))[:int(sys.argv[3]) if len(sys.argv)>3 else 20]:
    print(f'{(B[k]-A[k])/1024:+8.0f} KB {B[k]/1024:9.0f} KB  {k}')
