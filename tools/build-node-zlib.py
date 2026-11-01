#!/usr/bin/env python3
"""Build matching Node zlib on Linux x86-64, preserving its source and SIMD flags."""
import argparse,concurrent.futures,hashlib,json,platform,re,shutil,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--node-source',type=Path,required=True);p.add_argument('--prefix',type=Path,required=True);p.add_argument('--jobs',type=int,default=4);p.add_argument('--release-source',action='store_true',help='accept an extracted Node release source tree without Git metadata');p.add_argument('--expected-node-version',help='required with --release-source (for example v26.12.1)');a=p.parse_args()
if platform.system()!='Linux'or platform.machine()not in ('x86_64','AMD64'):p.error('this matching build recipe currently targets Linux x86-64')
if a.jobs<1:p.error('jobs must be positive')
from node_source_identity import resolve_node_source
repo=Path(__file__).resolve().parents[1];node=a.node_source.resolve();source_identity=resolve_node_source(node,repo,p,release_source=a.release_source,expected_node_version=a.expected_node_version)
zlib=node/'deps/zlib';gn=(zlib/'BUILD.gn').read_text();version=re.search(r'^#define ZLIB_VERSION "([^"]+)"',(zlib/'zlib.h').read_text(),re.M)[1]
def sources(name):
 block=re.search(r'(?:component|source_set)\("'+name+r'"\).*?sources = \[(.*?)\]',gn,re.S)[1]
 return [zlib/n for n in re.findall(r'"([^"]+\.c)"',block)]
inputs=list(dict.fromkeys(sources('zlib')+sources('zlib_data_chunk_simd')+sources('zlib_adler32_simd')))
output=a.prefix.resolve();objects=output/'obj';lib=output/'lib';pc=lib/'pkgconfig'
for path in (objects,pc):path.mkdir(parents=True,exist_ok=True)
defines=['HAVE_HIDDEN','ZLIB_IMPLEMENTATION','DEFLATE_SLIDE_HASH_SSE2','INFLATE_CHUNK_READ_64LE','INFLATE_CHUNK_SIMD_SSE2','ADLER32_SIMD_SSSE3','X86_NOT_WINDOWS']
def compile(path):
 obj=objects/(str(path.relative_to(zlib)).replace('/','_')+'.o');command=['gcc','-O3','-fPIC','-I'+str(zlib),*('-D'+d for d in defines)]
 if path.name=='adler32_simd.c':command+=['-mssse3']
 command+=['-c',str(path),'-o',str(obj)]
 with obj.with_suffix('.log').open('w')as f:subprocess.run(command,stdout=f,stderr=subprocess.STDOUT,check=True)
 return obj
with concurrent.futures.ThreadPoolExecutor(max_workers=a.jobs)as pool:compiled=list(pool.map(compile,inputs))
archive=lib/'libquench_node_zlib.a';subprocess.run(['ar','rcs',str(archive),*map(str,compiled)],check=True)
include=output/'include';include.mkdir(exist_ok=True)
for name in ('zlib.h','zconf.h'):shutil.copy2(zlib/name,include/name)
licenses=output/'share/licenses/zlib';licenses.mkdir(parents=True,exist_ok=True);shutil.copy2(zlib/'LICENSE',licenses/'LICENSE')
(pc/'quench-node-zlib.pc').write_text(f'prefix={output}\nlibdir=${{prefix}}/lib\nincludedir=${{prefix}}/include\nName: quench-node-zlib\nDescription: Matching Node native zlib\nVersion: {version}\nLibs: -L${{libdir}} -lquench_node_zlib\nCflags: -I${{includedir}}\n')
build_files=[zlib/'BUILD.gn',zlib/'zlib.h',zlib/'zconf.h',node/'src/node_version.h']
provenance=dict(**source_identity,codec='zlib',version=version,defines=defines,source_sha256={str(path.relative_to(node)):hashlib.sha256(path.read_bytes()).hexdigest()for path in inputs},build_input_sha256={str(path.relative_to(node)):hashlib.sha256(path.read_bytes()).hexdigest()for path in build_files},archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(),compiler=subprocess.check_output(['gcc','--version'],text=True).splitlines()[0],platform=platform.platform())
(output/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n');print('Built matching zlib',version,'objects',len(inputs))
