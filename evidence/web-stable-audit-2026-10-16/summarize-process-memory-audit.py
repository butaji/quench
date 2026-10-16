import collections,hashlib,json,os,pathlib,re
base=pathlib.Path('/workspace/quench');audit=base/'work/process-memory-stable-audit';root=base/'work/node_modules/node-stable-conformance'
report=json.loads((audit/'results.json').read_text());before=json.loads((audit/'source-before.json').read_text());qualification=json.loads((audit/'qualification-before.json').read_text())
changes=[]
for relative,expected in before.items():
 p=root/relative
 try:
  actual={'mode':p.lstat().st_mode}
  if p.is_symlink():actual['link']=os.readlink(p)
  elif p.is_file():
   actual['bytes']=p.stat().st_size
   if actual['bytes']==expected.get('bytes'):actual['sha256']=hashlib.sha256(p.read_bytes()).hexdigest()
  else:actual['kind']='missing-or-nonfile'
 except OSError as error:actual={'error':str(error)}
 if actual!=expected:changes.append({'path':relative,'before':expected,'after':actual})
(audit/'source-changes.json').write_text(json.dumps(changes,indent=2)+'\n')
counts=collections.Counter(row['status'] for row in report['results']);blockers=collections.Counter()
for row in report['results']:
 if row['status']!='fail':continue
 observation=row.get('observation',{}).get('Ok',{});worker=observation.get('worker') or {};reason=worker.get('reason','')
 lines=reason.splitlines();key=next((line for line in lines if re.match(r'^(?:[A-Za-z]*Error|Error)(?:\s|\[|:)',line)),lines[0] if lines else 'No worker diagnostic')
 blockers[key]+=1
identities={}
for name,metadata in qualification['binaries'].items():identities[name]=hashlib.sha256(pathlib.Path(metadata['path']).read_bytes()).hexdigest()==metadata['sha256']
summary=dict(source_commit=qualification['source_commit'],fixture_archive_sha256=qualification['archive_sha256'],counts=dict(counts),total=len(report['results']),source_files_unchanged=not changes,changed_existing_files=len(changes),binaries_unchanged=identities,diagnostic_only=True,total_node_parity=False,benchmark_win=False,notes=['Runner node_version is the frozen Quench compatibility version on PATH, not the oracle version.','tests_node_commit identifies the embedded repository pin, not the selected stable fixture checkout; the archive and source manifest identify that checkout.','This run uses two workers and a 30-second case deadline with concurrent Callgrind and compilation activity; compare individual observations, not timing or unqualified pass-count gains.','All literal node child commands resolve to the frozen Quench CLI.'],first_blockers=[dict(reason=reason,count=count)for reason,count in blockers.most_common(30)],memory_events_after=pathlib.Path('/sys/fs/cgroup/memory.events').read_text())
(audit/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:v for k,v in summary.items()if k in ['counts','total','source_files_unchanged','changed_existing_files','binaries_unchanged']},indent=2))
