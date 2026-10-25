#!/usr/bin/env python3
"""Build Brotli from the matching Node checkout for native compression handles."""
import argparse,ast,concurrent.futures,hashlib,json,platform,re,shutil,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--node-source',type=Path,required=True);p.add_argument('--prefix',type=Path,required=True);p.add_argument('--jobs',type=int,default=4);p.add_argument('--release-source',action='store_true',help='accept an extracted Node release source tree without Git metadata');p.add_argument('--expected-node-version',help='required with --release-source (for example v26.12.1)');a=p.parse_args()
if platform.system()!='Linux':p.error('this recipe currently supports Linux')
if a.jobs<1:p.error('jobs must be positive')
from node_source_identity import resolve_node_source
repo=Path(__file__).resolve().parents[1];node=a.node_source.resolve();source_identity=resolve_node_source(node,repo,p,release_source=a.release_source,expected_node_version=a.expected_node_version)
source=node/'deps/brotli';gyp=ast.literal_eval((source/'brotli.gyp').read_text());inputs=[source/name for name in gyp['variables']['brotli_sources']]
header=(source/'c/common/version.h').read_text();version='.'.join(re.search(r'^#define BROTLI_VERSION_'+name+r' (\d+)',header,re.M)[1]for name in ['MAJOR','MINOR','PATCH'])
output=a.prefix.resolve();objects=output/'obj';lib=output/'lib';pc=lib/'pkgconfig'
for d in (objects,pc):d.mkdir(parents=True,exist_ok=True)
def compile(path):
 obj=objects/(str(path.relative_to(source)).replace('/','_')+'.o');command=['gcc','-O3','-fPIC','-DOS_LINUX','-I'+str(source/'c/include'),'-c',str(path),'-o',str(obj)]
 with obj.with_suffix('.log').open('w')as f:subprocess.run(command,stdout=f,stderr=subprocess.STDOUT,check=True)
 return obj
with concurrent.futures.ThreadPoolExecutor(max_workers=a.jobs)as pool:compiled=list(pool.map(compile,inputs))
archive=lib/'libquench_node_brotli.a';subprocess.run(['ar','rcs',str(archive),*map(str,compiled)],check=True)
shutil.copytree(source/'c/include',output/'include',dirs_exist_ok=True);licenses=output/'share/licenses/brotli';licenses.mkdir(parents=True,exist_ok=True);shutil.copy2(source/'LICENSE',licenses/'LICENSE')
(pc/'quench-node-brotli.pc').write_text(f'prefix={output}\nlibdir=${{prefix}}/lib\nincludedir=${{prefix}}/include\nName: quench-node-brotli\nDescription: Matching Node Brotli codec\nVersion: {version}\nLibs: -L${{libdir}} -lquench_node_brotli -lm\nCflags: -I${{includedir}}\n')
build_files=[source/'brotli.gyp',source/'c/common/version.h',node/'src/node_version.h']
provenance=dict(**source_identity,codec='brotli',version=version,source_sha256={str(path.relative_to(node)):hashlib.sha256(path.read_bytes()).hexdigest()for path in inputs},build_input_sha256={str(path.relative_to(node)):hashlib.sha256(path.read_bytes()).hexdigest()for path in build_files},archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),compiler=subprocess.check_output(['gcc','--version'],text=True).splitlines()[0],platform=platform.platform());(output/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n');print('Built matching Brotli',version,'objects',len(inputs))
