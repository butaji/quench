from pathlib import Path
import concurrent.futures, json, os, re, subprocess
repo=Path('/workspace/quench');out=repo/'target/npm-capabilities-final-guard';raw=out/'raw.txt'
rows={}
for line in raw.read_text().splitlines():
    side,fixture,n,ir=line.split();assert int(ir)>0;rows[(side,fixture,int(n))]=int(ir)
plan={'crypto':1,'deltablue':10,'earley-boyer':1,'navier-stokes':1,'raytrace':3,'regexp':1,'richards':25,'splay':75}
missing=[(side,fx,n) for fx,count in plan.items() for n in [0,count] for side in ['base','cand'] if (side,fx,n) not in rows]
env=dict(os.environ,VALGRIND_LIB='/workspace/.local/usr/libexec/valgrind')
def run(item):
    side,fx,n=item;binary='/workspace/scratch/npm300-clock-candidate' if side=='base' else '/workspace/scratch/npm-capabilities-final-candidate'
    p=subprocess.run(['/workspace/.local/usr/bin/valgrind','--tool=callgrind','--callgrind-out-file=/dev/null',binary,str(out/f'{fx}-{n}.js')],cwd=repo,env=env,capture_output=True,text=True,timeout=1200)
    assert p.returncode==0 and p.stdout==f'done {fx} {n}\n',(item,p.returncode,p.stdout,p.stderr)
    match=re.search(r'I\s+refs:\s+([\d,]+)',p.stderr);assert match,item
    return item,int(match[1].replace(',',''))
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    for item,count in pool.map(run,missing):
        rows[item]=count
        with raw.open('a') as file:file.write(f'{item[0]} {item[1]} {item[2]} {count}\n')
        print(item,count,flush=True)
assert len(rows)==32
summary=[]
for fx,n in plan.items():
    base=(rows['base',fx,n]-rows['base',fx,0])/n;cand=(rows['cand',fx,n]-rows['cand',fx,0])/n
    summary.append({'fixture':fx,'runs':n,'startup_base':rows['base',fx,0],'startup_candidate':rows['cand',fx,0],'per_run_base':base,'per_run_candidate':cand,'delta_percent':100*(cand-base)/base})
Path('/workspace/scratch/npm-capabilities-final-guard.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2),flush=True)
