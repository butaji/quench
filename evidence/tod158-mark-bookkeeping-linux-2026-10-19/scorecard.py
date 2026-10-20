#!/usr/bin/env python3
# scorecard.py BASE CAND ROUNDS [suite,...] : interleaved quench-node pairs across the ecosystem.
# Suites: v8 (V8-v7), js3 (JetStream, reduced iterations), coremark (Wasm), fw (framework servers).
# Each run is its own process under spawn (posix_spawn + wait4 maxrss). Higher-is-better metric:
# Score where the workload prints one, else 1/wall. Prints per-workload paired ratios and RSS medians.
import os, re, statistics as st, subprocess, sys, json
S = os.path.dirname(os.path.abspath(__file__)); Q = '/home/user/quench'
base, cand, rounds = sys.argv[1], sys.argv[2], int(sys.argv[3])
suites = (sys.argv[4] if len(sys.argv) > 4 else 'v8,coremark,fw,js3').split(',')
JS3_ITER = os.environ.get('JS3_ITER', '15')
V8 = 'crypto deltablue earley-boyer navier-stokes raytrace regexp richards splay'.split()
JS3 = os.environ.get('JS3', 'UniPoker acorn-wtb async-fs babel-minify-wtb babel-wtb babylon-wtb bigint-noble-ed25519 '
      'chai-wtb doxbee-async doxbee-promise espree-wtb esprima-next-wtb hash-map js-tokens json-parse-inspector '
      'json-stringify-inspector lazy-collections postcss-wtb prettier-wtb source-map-wtb sqlite3-wasm '
      'stanford-crypto-aes stanford-crypto-pbkdf2 stanford-crypto-sha256 sync-fs typescript-lib validatorjs '
      'web-ssr zlib-wasm').split()
FW = 'express koa fastify hono h3'.split()
work = []
if 'v8' in suites: work += [(f'v8/{b}', [f'{S}/mat/{b}.js'], Q, r'^Score: (\S+)') for b in V8]
if 'coremark' in suites:
    work.append(('wasm/coremark', [f'{Q}/evidence/wasm-coremark-quench-node-linux-2026-10-17/run-js.mjs',
                 f'{S}/cm/coremark-minimal-mvp.wasm'], Q, r'Score: (\S+)'))
if 'fw' in suites: work += [(f'fw/{s}', ['driver.cjs', f'scenarios/{s}.cjs'], f'{Q}/tests/frameworks', None) for s in FW]
if 'js3' in suites:
    work += [(f'js3/{t}', [f'{S}/js3-prelude.js', 'quench-bench/jetstream', '--no-prefetch', f'--test={t}',
              f'--iteration-count={JS3_ITER}'], Q, r'Overall Score\s+(\S+)') for t in JS3]
of = f'{S}/sc.{os.getpid()}'
def run(binary, args, cwd, pat):
    r = subprocess.run([f'{S}/spawn', of, binary, *args], capture_output=True, text=True, cwd=cwd).stdout.split()
    rss, wall, code = int(r[0]) / 1024, float(r[1]), int(r[2]); txt = open(of).read()
    m = re.search(pat, txt, re.M) if pat else None
    ok = code == 0 and (m is not None if pat else True)
    return (float(m.group(1)) if m else 1 / wall) if ok else float('nan'), rss, wall
out = {}
for name, args, cwd, pat in work:
    rows = []
    for i in range(rounds):
        order = [(0, base), (1, cand)] if i % 2 == 0 else [(1, cand), (0, base)]
        res = {}
        for k, b in order: res[k] = run(b, args, cwd, pat)
        rows.append(res)
    x = [r[0] for r in rows]; y = [r[1] for r in rows]
    ratios = [c[0] / b[0] for b, c in zip(x, y)]
    ok = all(v == v for v in ratios)
    rec = dict(ratio=st.median(ratios) if ok else None, wins=sum(c[0] > b[0] for b, c in zip(x, y)),
               rss_base=st.median(p[1] for p in x), rss_cand=st.median(p[1] for p in y),
               wall_base=st.median(p[2] for p in x), wall_cand=st.median(p[2] for p in y))
    out[name] = rec
    speed = f"x{rec['ratio']:.3f}" if ok else 'FAIL'
    print(f"{name:32} speed {speed} wins {rec['wins']}/{rounds}  "
          f"RSS {rec['rss_base']:7.1f} -> {rec['rss_cand']:7.1f} MB  wall {rec['wall_base']:.1f}s", flush=True)
def geo(v): v = [a for a in v if a]; return st.geometric_mean(v) if v else float('nan')
for suite in sorted({n.split('/')[0] for n in out}):
    sel = [r for n, r in out.items() if n.startswith(suite + '/')]
    print(f"== {suite:9} speed geomean x{geo([r['ratio'] for r in sel]):.3f}  RSS ratio geomean "
          f"x{geo([r['rss_cand'] / r['rss_base'] for r in sel]):.3f}  ({len(sel)} workloads)")
json.dump(out, open(os.environ.get('SC_JSON', f'{S}/scorecard-last.json'), 'w'), indent=1)
