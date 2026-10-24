import subprocess,os,re,sys,tempfile,shutil,statistics as st
S=os.path.dirname(os.path.abspath(__file__)); R=S+'/rss-exec'; B=sys.argv[1]; runs=int(sys.argv[2])
def rss(script,m):
    e=dict(os.environ,QUENCH_MEMORY_POLICY='memory-first')
    o=subprocess.run([R,'--evict',B,'--',B,*m,script],env=e,capture_output=True,text=True)
    return int(re.search(r'maxrss_kib=(\d+)',o.stdout+o.stderr).group(1))
for f in sys.argv[3].split(','):
  for m in ([],['--jit']):
    c=[];w=[]
    for r in range(runs):
        d=tempfile.mkdtemp(dir=S+'/fresh'); p=d+f'/{f}.js'; shutil.copy(f'{S}/v8fx/{f}.js',p)
        c.append(rss(p,m)); w.append(rss(p,m)); shutil.rmtree(d)
    print(f"{f:14}{''.join(m):6} cold med {st.median(c):.0f} max {max(c)}  warm med {st.median(w):.0f} max {max(w)}  d {st.median(c)-st.median(w):+.0f}",flush=True)
