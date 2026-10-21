import json,sys,glob,os
for f in sorted(glob.glob(sys.argv[1]+'/*.txt')):
    rows=[json.loads(l) for l in open(f)]
    r=max(rows,key=lambda r:sum(r['property_classes'])+sum(r['element_classes']))
    w,n=r['objects_by_length_wide_narrow']; tot=sum(w)+sum(n)
    pct=lambda x: f"{x*100/max(1,tot):4.1f}"
    # 24 -> 16 bytes for objects with <=2 narrow props
    save=8*sum(n[:3])/2**20
    print(f"{os.path.basename(f)[:-4]:25} objs={tot:>8} len<=2 narrow={pct(sum(n[:3]))}% wide={pct(sum(w[:3]))}% len3-4={pct(n[3]+n[4]+w[3]+w[4])}% len5+={pct(sum(w[5:])+sum(n[5:]))}% 16B-record save={save:4.2f}MB")
