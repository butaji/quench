import re,sys
txt=open(sys.argv[1]).read().split('\n'); snaps=[];cur=None
for l in txt:
    if l.startswith('snapshot='): cur={'tree':[]}; snaps.append(cur)
    elif cur is None: continue
    elif l.startswith('mem_heap_B='): cur['heap']=int(l[11:])
    elif re.match(r'^ *n\d+: ',l): cur['tree'].append(l)
RES=64<<20; best=0
for s in snaps:
    if not s['tree']: continue
    res=0
    for l in s['tree']:
        m=re.match(r'^( *)n(\d+): (\d+) ',l)
        if len(m.group(1))==1 and int(m.group(3))>=RES: res+=int(m.group(3))
    best=max(best,s['heap']-res)
print('peak malloc excluding reservations KiB',best//1024)
