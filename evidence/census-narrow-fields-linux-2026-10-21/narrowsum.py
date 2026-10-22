import json,sys,glob,os
NARROW={'int31','constant','reference'}
for f in sorted(glob.glob(sys.argv[1]+'/*.txt')):
    rows=[json.loads(l) for l in open(f)]
    r=max(rows,key=lambda r:sum(r['property_classes'])+sum(r['element_classes']))
    names=r['classes']; out=[]
    for part in ('property','element'):
        c=dict(zip(names,r[part+'_classes'])); tot=sum(c.values())
        fit=sum(c[n] for n in NARROW)
        out.append(f"{part}: slots={tot:>8} fit={fit*100/max(1,tot):5.1f}% dbl={c['double']*100/max(1,tot):4.1f}% dblint={c['double_int31']*100/max(1,tot):4.1f}% narrow-containers={r[part+'_narrow_container_slots']*100/max(1,tot):5.1f}% of slots")
    save=4*(r['property_narrow_container_slots']+r['element_narrow_container_slots'])/2**20
    print(f"{os.path.basename(f)[:-4]:25} {r['phase']:12} save~{save:5.2f}MB | "+' | '.join(out))
