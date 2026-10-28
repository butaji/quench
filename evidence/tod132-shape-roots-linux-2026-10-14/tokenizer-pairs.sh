#!/usr/bin/env bash
# From repository root: tokenizer-pairs.sh BASELINE CANDIDATE NEW_OUTPUT_DIR
set -euo pipefail
mkdir "$3"
cat quench-bench/jetstream/generators/js-tokens.js > "$3/tokens-10.js"
cat >> "$3/tokens-10.js" <<'JS'
let count = 0;
for (let pass = 0; pass < 10; pass++) {
  for (const token of jsTokens(jsTokensSourceCode)) count++;
}
console.log(count);
JS
python3 -B - "$1" "$2" "$3" <<'PY'
import hashlib, importlib.util, json, os, pathlib, platform, random, statistics, sys
# Reuse the repository's existing wait4 measurement and paired-bootstrap tool.
spec = importlib.util.spec_from_file_location('pairs', 'evidence/task61-shape-compaction-pairs-linux-2026-10-09.py')
pairs = importlib.util.module_from_spec(spec); spec.loader.exec_module(pairs)
binaries = {side:pathlib.Path(path).resolve() for side,path in zip(('baseline','candidate'),sys.argv[1:3])}
out = pathlib.Path(sys.argv[3]).resolve(); script = out/'tokens-10.js'; rounds=[]
for side,binary in binaries.items():
    warm = pairs.run_one(binary, script, os.environ.copy(), out, 'warm-'+side)
    assert warm['exit_code']==0 and not warm['timed_out'] and warm['stdout'].strip()=='32990', warm
for index in range(pairs.PAIR_COUNT):
    order = ['baseline','candidate'] if index%2==0 else ['candidate','baseline']
    samples = {side:pairs.run_one(binaries[side],script,os.environ.copy(),out,f'{index:02}-{side}') for side in order}
    assert all(s['exit_code']==0 and not s['timed_out'] and s['stdout'].strip()=='32990' for s in samples.values()), samples
    rounds.append({'round':index,'order':order,'samples':samples})
summary={}
for metric in ('wall_ns','maximum_rss_bytes'):
    base=[r['samples']['baseline'][metric] for r in rounds]; cand=[r['samples']['candidate'][metric] for r in rounds]
    summary[metric]={'baseline_median':statistics.median(base),'candidate_median':statistics.median(cand),'delta_percent':100*(statistics.median(cand)/statistics.median(base)-1),'paired_interval':pairs.paired_bootstrap(base,cand,random.Random(pairs.SEED))}
report={'host':platform.platform(),'profile':'production','binary_sha256':{side:pairs.file_hash(binary) for side,binary in binaries.items()},'script_sha256':pairs.file_hash(script),'oracle_stdout':'32990','summary':summary,'rounds':rounds}
(out/'pairs.json').write_text(json.dumps(report,indent=2)+'\n'); print(json.dumps(summary,indent=2))
PY
