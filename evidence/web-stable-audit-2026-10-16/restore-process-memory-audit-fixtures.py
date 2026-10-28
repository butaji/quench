import hashlib,json,os,pathlib,shutil,stat,tarfile
base=pathlib.Path('/workspace/quench');audit=base/'work/process-memory-stable-audit';root=base/'work/node_modules/node-stable-conformance';changes=json.loads((audit/'source-changes.json').read_text());before=json.loads((audit/'source-before.json').read_text());archive=base/'work/node-v26.12.0.tar.xz'
assert hashlib.sha256(archive.read_bytes()).hexdigest()=='ce59f9038dda0e637656fb1c36cf52c07c637922fd81d419dbba2ec129afbd35'
with tarfile.open(archive) as tar:
 for change in changes:
  relative=change['path'];p=root/relative;backup=audit/'fixture-mutations'/relative;backup.parent.mkdir(parents=True,exist_ok=True)
  if p.is_file():shutil.copy2(p,backup)
  member=tar.getmember('node-v26.12.0/'+relative);assert member.isfile()
  original=tar.extractfile(member).read();expected=change['before'];assert hashlib.sha256(original).hexdigest()==expected['sha256']
  p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(original);p.chmod(stat.S_IMODE(expected['mode']))
errors=[]
for relative,expected in before.items():
 p=root/relative
 try:
  actual={'mode':p.lstat().st_mode}
  if p.is_symlink():actual['link']=os.readlink(p)
  elif p.is_file():actual.update(bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest())
  if actual!=expected:errors.append(relative)
 except OSError:errors.append(relative)
report={'restored_paths':[r['path']for r in changes],'all_original_files_match_initial_manifest':not errors,'mismatches':errors,'initial_run_mutations_retained':True,'input_identity_during_run_not_claimed':True}
(audit/'source-restoration.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));assert not errors
