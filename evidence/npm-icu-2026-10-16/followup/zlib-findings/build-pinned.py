from pathlib import Path
import subprocess,concurrent.futures,json
root=Path('/workspace/scratch/node26-12-oracle/deps/zlib');out=Path('/workspace/shared/node-zlib-draft');out.mkdir(exist_ok=True)
files=['adler32.c','compress.c','cpu_features.c','crc32.c','deflate.c','gzclose.c','gzlib.c','gzread.c','gzwrite.c','infback.c','inffast.c','inftrees.c','trees.c','uncompr.c','zutil.c','contrib/optimizations/inffast_chunk.c','contrib/optimizations/inflate.c','adler32_simd.c']
def build(name):
 target=out/(name.replace('/','__')+'.o')
 args=['gcc','-O2','-fPIC','-DZ_PREFIX','-DHAVE_HIDDEN','-DADLER32_SIMD_SSSE3','-DX86_NOT_WINDOWS','-DINFLATE_CHUNK_SIMD_SSE2','-DINFLATE_CHUNK_READ_64LE','-DDEFLATE_SLIDE_HASH_SSE2','-I'+str(root),'-I'+str(root/'contrib/optimizations')]
 if name=='adler32_simd.c':args+=['-mssse3']
 args+=['-c',str(root/name),'-o',str(target)]
 r=subprocess.run(args,capture_output=True,text=True);(out/(name.replace('/','__')+'.log')).write_text(r.stdout+r.stderr)
 if r.returncode:raise RuntimeError(name+': '+r.stderr[:1500])
 return target
try:
 with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:objects=list(pool.map(build,files))
 subprocess.run(['ar','rcs',str(out/'libnodezlib.a'),*map(str,objects)],check=True)
 (out/'provenance.json').write_text(json.dumps({'node_source':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'files':files,'flags':'Node zlib.gyp Linux x64 SIMD flags, plus Z_PREFIX for isolated linking'},indent=2)+'\n')
except Exception:
 (out/'build.exit').write_text('1\n');raise
else:(out/'build.exit').write_text('0\n')
