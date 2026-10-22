import hashlib,json,os,pathlib,shutil,subprocess
base=pathlib.Path('/workspace/quench');audit=base/'work/process-memory-stable-audit';audit.mkdir();binary=audit/'bin';binary.mkdir()
source=base/'work/node_modules/node-stable-conformance';source.mkdir()
archive=base/'work/node-v26.12.0.tar.xz';sha=hashlib.sha256(archive.read_bytes()).hexdigest();assert sha=='ce59f9038dda0e637656fb1c36cf52c07c637922fd81d419dbba2ec129afbd35'
subprocess.run(['tar','-xJf',str(archive),'--strip-components=1','-C',str(source)],check=True)
for name,path in [('quench-node',base/'work/quench-process-memory-qualified'),('run-parallel',base/'work/host-build/iteration/run-parallel')]:shutil.copy2(path,binary/name)
(binary/'node').symlink_to('quench-node')
def manifest(root):
 result={}
 for p in sorted(root.rglob('*')):
  if p.is_symlink():result[str(p.relative_to(root))]={'link':os.readlink(p),'mode':p.lstat().st_mode}
  elif p.is_file():result[str(p.relative_to(root))]={'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'mode':p.stat().st_mode,'bytes':p.stat().st_size}
 return result
(audit/'source-before.json').write_text(json.dumps(manifest(source),indent=2)+'\n')
metadata=dict(source_commit=subprocess.check_output(['git','rev-parse','4aa47e844'],cwd=base,text=True).strip(),archive_sha256=sha,fixture_root=str(source/'test/parallel'),oracle_version='Node v26.12.0',jobs=2,timeout_seconds=30,diagnostic_only=True,total_node_parity=False,concurrent_load='Two paired Callgrind workers; fixed instruction counts are not wall timing. Suite deadlines remain diagnostic.',literal_node_children_use_frozen_quench=True,binaries={})
for name in ['quench-node','run-parallel']:
 p=binary/name;metadata['binaries'][name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
for name,path in [('cpu_max','/sys/fs/cgroup/cpu.max'),('memory_max','/sys/fs/cgroup/memory.max'),('memory_events_before','/sys/fs/cgroup/memory.events')]:metadata[name]=pathlib.Path(path).read_text()
(audit/'qualification-before.json').write_text(json.dumps(metadata,indent=2)+'\n')
print('Frozen stable source and sibling binaries prepared',flush=True)
