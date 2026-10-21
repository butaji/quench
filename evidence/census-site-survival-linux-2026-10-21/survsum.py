import json,sys,glob,os
for f in sorted(glob.glob(sys.argv[1]+'/*.txt')):
    rows=[json.loads(l) for l in open(f)]
    h=rows[0]; names=h['kind_names']; k=h['died_young_died_old_live_by_kind']
    tot=[sum(x[i] for x in k) for i in range(3)]; alloc=sum(tot)
    per=' '.join(f"{n}:{(x[1]+x[2])*100/max(1,sum(x)):.0f}%/{sum(x)}" for n,x in zip(names,k) if sum(x)>1000)
    sv=h['survivors']
    print(f"{os.path.basename(f)[:-4]:26} alloc={alloc:>9} young-death={tot[0]*100/max(1,alloc):5.1f}% survive={sv*100/max(1,alloc):5.1f}% pretenurable={h['survivors_from_pretenurable_sites']*100/max(1,sv):5.1f}% of survivors | {per}")
