#!/usr/bin/env python3
"""Build Node's pinned ICU common/converter tables for a Linux little-endian host.

Example:
  python3 tools/build-node-icu.py --node-source /path/to/full/node --prefix /tmp/quench-icu
  PKG_CONFIG_PATH=/tmp/quench-icu/lib/pkgconfig cargo build -p quench-node

The Node checkout must match tests/node and include deps/icu-small. No downloaded
or system Unicode tables are substituted. This builds only ICU common/data;
Quench does not need the ICU internationalization/formatting library here.
"""
import argparse
import bz2
import concurrent.futures
import hashlib
import json
import platform
from pathlib import Path
import re
import shutil
import subprocess
import sys


def run(args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--node-source', type=Path, required=True)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--jobs', type=int, default=4)
    args = parser.parse_args()
    if sys.platform != 'linux' or sys.byteorder != 'little':
        parser.error('this recipe currently supports little-endian Linux hosts')
    if args.jobs < 1:
        parser.error('--jobs must be positive')
    repo = Path(__file__).resolve().parents[1]
    expected = subprocess.check_output(['git', 'ls-tree', 'HEAD', 'tests/node'], cwd=repo, text=True).split()[2]
    node = args.node_source.resolve()
    actual = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=node, text=True).strip()
    if actual != expected:
        parser.error(f'Node source {actual} does not match pinned tests/node {expected}')
    icu = node / 'deps/icu-small'
    source = icu / 'source'
    header = source / 'common/unicode/uvernum.h'
    if not header.is_file():
        parser.error('the pinned Node checkout must include deps/icu-small (expand its sparse checkout)')
    version = re.search(r'^#define U_ICU_VERSION "([^"]+)"', header.read_text(), re.M).group(1)
    major = version.split('.')[0]
    archive = source / f'data/in/icudt{major}l.dat.bz2'
    output = args.prefix.resolve()
    objects = output / 'obj'
    libraries = output / 'lib'
    pkgconfig = libraries / 'pkgconfig'
    for directory in [objects, pkgconfig]:
        directory.mkdir(parents=True, exist_ok=True)
    inputs = sorted((source / 'common').glob('*.cpp'))

    def compile_source(path):
        target = objects / path.with_suffix('.o').name
        command = ['g++', '-std=c++17', '-O2', '-fPIC', '-DU_STATIC_IMPLEMENTATION=1',
                   '-DU_COMMON_IMPLEMENTATION=1', '-DUCONFIG_NO_SERVICE=1', '-DU_ENABLE_DYLOAD=0',
                   '-DU_HAVE_STD_STRING=1', '-DUCONFIG_NO_BREAK_ITERATION=0', '-Wno-deprecated-declarations',
                   '-I' + str(source / 'common'), '-I' + str(source / 'i18n'), '-c', str(path), '-o', str(target)]
        with (objects / path.with_suffix('.log').name).open('wb') as log:
            run(command, stdout=log, stderr=subprocess.STDOUT)
        return target

    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        compiled = list(pool.map(compile_source, inputs))
    common_archive = libraries / 'libicuuc.a.next'
    common_archive.unlink(missing_ok=True)
    run(['ar', 'rcs', str(common_archive), *map(str, compiled)])
    common_archive.replace(libraries / 'libicuuc.a')
    data = bz2.decompress(archive.read_bytes())
    data_path = output / f'icudt{major}l.dat'
    data_path.write_bytes(data)
    marker = '%' if platform.machine() in ('aarch64', 'arm64') else '@'
    symbol = f'icudt{major}_dat'
    assembly = output / 'icudata.S'
    # Use a generated ASCII filename and an explicit assembler working directory.
    assembly.write_text(f'.section .rodata\n.balign 16\n.globl {symbol}\n.type {symbol}, {marker}object\n'
                        f'{symbol}:\n.incbin "{data_path.name}"\n.size {symbol}, .-{symbol}\n'
                        f'.section .note.GNU-stack,"",{marker}progbits\n')
    data_object = objects / 'icudata.o'
    run(['gcc', '-c', str(assembly), '-o', str(data_object)], cwd=output)
    run(['ar', 'rcs', str(libraries / 'libicudata.a'), str(data_object)])
    include = output / 'include/unicode'
    shutil.copytree(source / 'common/unicode', include, dirs_exist_ok=True)
    licenses = output / 'share/licenses/icu'
    licenses.mkdir(parents=True, exist_ok=True)
    shutil.copy2(icu / 'LICENSE', licenses / 'LICENSE')
    (pkgconfig / 'icu-uc.pc').write_text(
        f'prefix={output}\nlibdir=${{prefix}}/lib\nincludedir=${{prefix}}/include\n'
        f'Name: icu-uc\nDescription: Pinned Node ICU common and converter data\nVersion: {version}\n'
        'Libs: -L${libdir} -licuuc -licudata -lstdc++ -lpthread -lm -ldl\nCflags: -I${includedir}\n')
    provenance = {'node_source': actual, 'icu': version, 'common_objects': len(compiled),
                  'data_sha256': hashlib.sha256(data).hexdigest(), 'platform': platform.platform(),
                  'compiler': subprocess.check_output(['g++', '--version'], text=True).splitlines()[0]}
    (output / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    print(f'Built ICU {version}; set PKG_CONFIG_PATH={pkgconfig}')


if __name__ == '__main__':
    main()
