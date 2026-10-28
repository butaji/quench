import collections,hashlib,json,os,subprocess
from pathlib import Path
root=Path('/workspace/quench');out=root/'work/error-source-policy';env=os.environ.copy();env.pop('QUENCH_NODE_LIBRARY_ROOT',None);env['PATH']=str(root/'work/node-v26.12.0-linux-x64/bin')+':'+env['PATH'];rows=[]
for label,bundle in [('before',root/'work/shared-post-prepared-bin'),('candidate',root/'work/error-source-bin')]:
 env['QUENCH_NODE_TEST_EXECUTABLE']=str(bundle/'quench-node')
 for group in ['test-assert']:
  result=out/('family-'+label+'-'+group+'.json');command=[str(bundle/'run-parallel'),'--all','--parallel-dir','/workspace/work/node-stable-release/node-v26.12.0/test/parallel','--filter',group,'--jobs','4','--timeout-secs','30','--results',str(result)]
  with (out/('family-'+label+'-'+group+'.log')).open('w')as log:code=subprocess.run(command,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT).returncode
  data=json.loads(result.read_text());rows.append({'label':label,'group':group,'command':command,'exit':code,'counts':dict(collections.Counter(r['status']for r in data['results'])),'inventory_hash':data['inventory_hash']})
a={};b={}
for label,target in [('before',a),('candidate',b)]:
 for p in sorted(out.glob('family-'+label+'-*.json')):
  for r in json.loads(p.read_text())['results']:target[Path(r['fixture']).name]=r['status']
assert a.keys()==b.keys()
receipt={'runs':rows,'before':dict(collections.Counter(a.values())),'candidate':dict(collections.Counter(b.values())),'total':len(b),'gained':[n for n in sorted(b)if b[n]=='pass'and a[n]!='pass'],'lost':[n for n in sorted(b)if b[n]!='pass'and a[n]=='pass'],'skip_changes':{n:{'before':a[n],'after':b[n]}for n in sorted(b)if a[n]!=b[n]and'skip'in[a[n],b[n]]}}
(out/'family-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
