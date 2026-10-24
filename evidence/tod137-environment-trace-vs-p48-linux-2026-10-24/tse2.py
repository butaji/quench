import subprocess,os,sys,resource,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); BIN={}
for a in sys.argv[1].split(','):
    k,v=a.split('=',1); b,_,e=v.partition('@'); BIN[k]=(b,dict([e.split(':')]) if e else {}); runs=int(sys.argv[2])
SC='/home/user/quench/tests/npm-top300/scenario.mjs'; os.chdir(S+'/tse')
def run(be,p):
    b,env=be
    r0=resource.getrusage(resource.RUSAGE_CHILDREN)
    o=subprocess.run([b,SC,p],capture_output=True,text=True,env=dict(os.environ,**env))
    r1=resource.getrusage(resource.RUSAGE_CHILDREN)
    assert 'npm-behavior-ok' in o.stdout+o.stderr, (o.stdout+o.stderr)[-300:]
    return (r1.ru_utime-r0.ru_utime+r1.ru_stime-r0.ru_stime)*1000, r1.ru_minflt-r0.ru_minflt
for p in sys.argv[3].split(','):
    res={k:[] for k in BIN}
    for k in BIN: run(BIN[k],p)
    for r in range(runs):
        for k in (list(BIN) if r%2==0 else list(BIN)[::-1]): res[k].append(run(BIN[k],p))
    print(p, ' | '.join(f"{k}: cpu {st.median(x[0] for x in v):.0f} ms faults {st.median(x[1] for x in v):.0f}" for k,v in res.items()),flush=True)
