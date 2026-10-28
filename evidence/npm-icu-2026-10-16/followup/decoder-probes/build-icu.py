from pathlib import Path
import bz2,concurrent.futures,json,subprocess,hashlib
root=Path('/workspace/scratch/node26-12-oracle/deps/icu-small/source')
out=Path('/workspace/shared/icu78-node');out.mkdir(exist_ok=True);(out/'obj').mkdir(exist_ok=True);(out/'lib/pkgconfig').mkdir(parents=True,exist_ok=True)
source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();assert source=='26e72d85a611e222e3f06025de8a3c496923bacd'
def build(p):
 target=out/'obj'/p.with_suffix('.o').name
 args=['g++','-std=c++17','-O2','-fPIC','-DU_STATIC_IMPLEMENTATION=1','-DU_COMMON_IMPLEMENTATION=1','-DUCONFIG_NO_SERVICE=1','-DU_ENABLE_DYLOAD=0','-DU_HAVE_STD_STRING=1','-DUCONFIG_NO_BREAK_ITERATION=0','-Wno-deprecated-declarations','-I'+str(root/'common'),'-I'+str(root/'i18n'),'-c',str(p),'-o',str(target)]
 result=subprocess.run(args,capture_output=True,text=True)
 (out/'obj'/p.with_suffix('.log').name).write_text(result.stdout+result.stderr)
 if result.returncode:raise RuntimeError(str(p)+': '+result.stderr[:1000])
 return target
try:
 with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool: objects=list(pool.map(build,sorted((root/'common').glob('*.cpp'))))
 subprocess.run(['ar','rcs',str(out/'lib/libicuuc.a'),*map(str,objects)],check=True)
 data=bz2.decompress((root/'data/in/icudt78l.dat.bz2').read_bytes());(out/'icudt78l.dat').write_bytes(data)
 (out/'icudata.S').write_text('.section .rodata\n.balign 16\n.globl icudt78_dat\n.type icudt78_dat, @object\nicudt78_dat:\n.incbin "'+str(out/'icudt78l.dat')+'"\n.size icudt78_dat, .-icudt78_dat\n.section .note.GNU-stack,"",@progbits\n')
 subprocess.run(['gcc','-c',str(out/'icudata.S'),'-o',str(out/'obj/icudata.o')],check=True)
 subprocess.run(['ar','rcs',str(out/'lib/libicudata.a'),str(out/'obj/icudata.o')],check=True)
 (out/'lib/pkgconfig/icu-uc.pc').write_text('prefix='+str(out)+'\nlibdir=${prefix}/lib\nincludedir='+str(root/'common')+'\nName: icu-uc\nDescription: Pinned Node ICU common and converter data\nVersion: 78.3\nLibs: -L${libdir} -licuuc -licudata -lstdc++ -lpthread -lm -ldl\nCflags: -I${includedir}\n')
 (out/'provenance.json').write_text(json.dumps({'node_source':source,'icu':'78.3','source':str(root),'common_objects':len(objects),'data_sha256':hashlib.sha256(data).hexdigest(),'compiler':subprocess.check_output(['g++','--version'],text=True).splitlines()[0]},indent=2)+'\n')
except Exception:
 (out/'build.exit').write_text('1\n');raise
else:(out/'build.exit').write_text('0\n')
