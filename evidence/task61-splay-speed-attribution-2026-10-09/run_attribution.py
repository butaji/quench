#!/usr/bin/env python3
"""Post-M1 Splay operation screen and one-run physical opcode census."""
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
M1 = ROOT / "target/pinned/d28c6a722fd77b3a52a2a5c31ec455fcf0207609e393ca7f82f96ffd5255b962/quench-node"
PROFILE = ROOT / "target/pinned/042e54c915133399799219b3c6656b6e5c8d15882845eb198b61d649f74fc34f/quench-node"
SPLAY = ROOT / "target/iteration/v8-v7-opcode-census-2026-10-08/splay.js"
BASELINE = json.loads((ROOT / "tasks/evidence/task61-per-op-budget-2026-10-09.json").read_text())

SCRIPTS = {
    "local-store": (2_000_000,
        "var r=0; for(var i=0;i<N;i++) r=i;",
        "if(N ? r!==N-1 : r!==0) throw new Error('local-store check');"),
    "field-read": (2_000_000,
        "var o={x:3}, r=0; for(var i=0;i<N;i++) r=o.x;",
        "if(N ? r!==3 : r!==0) throw new Error('field-read check');"),
    "field-write": (2_000_000,
        "var o={x:0}; for(var i=0;i<N;i++) o.x=i;",
        "if(N ? o.x!==N-1 : o.x!==0) throw new Error('field-write check');"),
    "local-call": (1_000_000,
        "function fn(x){return x+1;} var r=0; for(var i=0;i<N;i++) r=fn(i);",
        "if(N ? r!==N : r!==0) throw new Error('local-call check');"),
    "construct": (200_000,
        "function P(x){this.x=x;} var r=null; for(var i=0;i<N;i++) r=new P(i);",
        "if(N ? r.x!==N-1 : r!==null) throw new Error('construct check');"),
    "object-literal-2": (200_000,
        "var result=null; for(var i=0;i<N;i++) result={left:i,right:i+1};",
        "if(N ? (!result || result.left!==N-1 || result.right!==N) : result!==null) throw new Error('literal-2 check');"),
    "object-literal-nested-array": (2_000_000,
        "var value='payload', result=null; for(var i=0;i<N;i++) result={array:[0,1,2,3,4,5,6,7,8,9],string:value};",
        "if(N ? (!result || result.array.length!==10 || result.array[9]!==9 || result.string!==value) : result!==null) throw new Error('nested literal check');"),
}

PATTERNS = {
    "instructions_retired": re.compile(r"(?m)^\s*(\d+)\s+instructions retired\s*$"),
    "cycles_elapsed": re.compile(r"(?m)^\s*(\d+)\s+cycles elapsed\s*$"),
    "maximum_rss_bytes": re.compile(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$"),
    "elapsed_seconds": re.compile(r"(?m)^\s*([0-9.]+)\s+real\b"),
}

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def write_scripts():
    scripts = {}
    for name, (iterations, body, check) in SCRIPTS.items():
        pair = {}
        for phase, count in (("active", iterations), ("zero", 0)):
            path = OUT / f"{name}-{phase}.cjs"
            path.write_text(f"(function(){{var N={count};{body}{check}}})();\n")
            pair[phase] = path
        scripts[name] = pair
    return scripts

def measure(binary, engine_args, env, script):
    command = ["/usr/bin/time", "-l", str(binary), *engine_args, str(script)]
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env={**os.environ, **env}, capture_output=True, text=True)
    report = result.stderr + result.stdout
    metrics = {}
    for name, pattern in PATTERNS.items():
        match = pattern.search(report)
        metrics[name] = (float(match.group(1)) if name == "elapsed_seconds" else int(match.group(1))) if match else None
    return {
        "command": command, "exit_code": result.returncode, "metrics": metrics,
        "stdout_sha256": hashlib.sha256(result.stdout.encode()).hexdigest(),
        "stderr_tail": result.stderr[-1600:], "wall_seconds": time.monotonic()-started,
    }

def opcode_census():
    source = SPLAY.read_text()
    marker = "let __quenchBenchSucceeded = true;"
    pos = source.rfind(marker)
    if pos < 0:
        raise RuntimeError("Splay fixture lacks expected harness marker")
    template = """(function(){var suites=BenchmarkSuite.suites;for(var si=0;si<suites.length;si++){var bs=suites[si].benchmarks;for(var bi=0;bi<bs.length;bi++){var b=bs[bi];b.Setup();if(RUN)b.run();}}})();"""
    phases = {}
    for phase, run in (("setup", "false"), ("setup_and_run", "true")):
        text = source[:pos] + template.replace("RUN", run)
        handle = tempfile.NamedTemporaryFile(mode="w", suffix="-splay.js", dir=OUT, delete=False)
        try:
            handle.write(text)
            handle.close()
            result = subprocess.run([str(PROFILE), handle.name], cwd=ROOT,
                env={**os.environ, "QUENCH_OPCODE_CENSUS":"1"}, capture_output=True, text=True)
        finally:
            Path(handle.name).unlink(missing_ok=True)
        if result.returncode:
            raise RuntimeError(f"profile census failed in {phase}: {result.stderr[-3000:]}")
        reports = [json.loads(line) for line in result.stderr.splitlines() if line.startswith("{")]
        dispatch = next((report for report in reports if report.get("kind")=="quench-dispatch-opcode-census"), None)
        if not dispatch:
            raise RuntimeError(f"missing dispatch census for {phase}")
        phases[phase] = {"total":dispatch["total"], "site_total":dispatch["site_total"], "counts":dispatch["counts"]}
    setup = phases["setup"]["counts"]
    work = phases["setup_and_run"]["counts"]
    per_run = {op: count-setup.get(op,0) for op,count in work.items() if count-setup.get(op,0)}
    selected = {op: per_run.get(op,0) for op in ("Call", "Construct", "GetField", "SetField", "MakeObject2", "SuperConstArrayObject2", "GetThisField", "SetThisFieldStrict")}
    return {
        "profile_binary":str(PROFILE.relative_to(ROOT)), "profile_binary_sha256":digest(PROFILE),
        "profile_binary_source_revision":"0f980819a5982fd27f09a83fee87d03ba77368ff (pre-M1; M1 changed only GC root derivation, not dispatch/counter code)",
        "fixture":str(SPLAY.relative_to(ROOT)), "fixture_sha256":digest(SPLAY),
        "method":"one Setup-only process and one Setup+run() process; subtract physical dispatch counters by opcode",
        "phases":phases, "one_run_total":phases["setup_and_run"]["total"]-phases["setup"]["total"],
        "one_run_opcode_counts":per_run, "requested_opcode_counts":selected,
    }

def main():
    if not M1.is_file() or not PROFILE.is_file() or not SPLAY.is_file():
        raise SystemExit("missing pinned M1, profile, or materialized Splay input")
    scripts = write_scripts()
    engines = {
        "quench": (M1, [], {}),
        "rqj": (Path(BASELINE["engines"]["rqj"]["binary"]), BASELINE["engines"]["rqj"]["args"], BASELINE["engines"]["rqj"]["environment"]),
        "qjs": (Path(BASELINE["engines"]["qjs"]["binary"]), BASELINE["engines"]["qjs"]["args"], BASELINE["engines"]["qjs"]["environment"]),
        "node_jitless": (Path(BASELINE["engines"]["node_jitless"]["binary"]), BASELINE["engines"]["node_jitless"]["args"], BASELINE["engines"]["node_jitless"]["environment"]),
        "bun_no_jit": (Path(BASELINE["engines"]["bun_no_jit"]["binary"]), BASELINE["engines"]["bun_no_jit"]["args"], BASELINE["engines"]["bun_no_jit"]["environment"]),
    }
    records=[]
    names=list(engines)
    for case_index,(case,pair) in enumerate(scripts.items()):
        iterations=SCRIPTS[case][0]
        for rep in range(1,4):
            shift=(case_index+rep-1)%len(names)
            for engine_name in names[shift:]+names[:shift]:
                binary,args,env=engines[engine_name]
                active=measure(binary,args,env,pair["active"])
                zero=measure(binary,args,env,pair["zero"])
                if active["exit_code"] or zero["exit_code"]:
                    raise RuntimeError(f"{case}/{engine_name}/{rep} failed: active={active}, zero={zero}")
                per={}
                for metric in ("instructions_retired","cycles_elapsed","elapsed_seconds"):
                    av,zv=active["metrics"][metric],zero["metrics"][metric]
                    per[metric]=None if av is None or zv is None else (av-zv)/iterations
                records.append({"case":case,"iterations":iterations,"engine":engine_name,"repetition":rep,"active":active,"zero":zero,"net_per_iteration":per})
            partial={"records":records}
            (OUT/"per-op.partial.json").write_text(json.dumps(partial,indent=2)+"\n")
            print(f"{case}: pair {rep}/3",flush=True)
    summary={}
    for case in SCRIPTS:
        summary[case]={}
        for name in engines:
            rows=[r for r in records if r["case"]==case and r["engine"]==name]
            summary[case][name]={}
            for metric in ("instructions_retired","cycles_elapsed","elapsed_seconds"):
                summary[case][name][metric]=sorted(r["net_per_iteration"][metric] for r in rows)[1]
        ref=min((name for name in engines if name!="quench"),key=lambda n:summary[case][n]["cycles_elapsed"])
        summary[case]["best_reference_by_cycles"]=ref
        summary[case]["quench_to_best_reference_cycles_ratio"]=summary[case]["quench"]["cycles_elapsed"]/summary[case][ref]["cycles_elapsed"]
    output={
        "schema":"quench.task61.splay-post-m1-speed-attribution.v1","date":"2026-10-09",
        "host":{"platform":platform.platform(),"machine":platform.machine()},
        "quench":{"binary":str(M1.relative_to(ROOT)),"sha256":digest(M1),"role":"pinned M1 production candidate"},
        "reference_binaries":{name:{"binary":str(binary),"sha256":digest(binary),"args":args,"environment":env} for name,(binary,args,env) in engines.items() if name!="quench"},
        "measurement":"3 repetitions; active minus zero process counters divided by fixed loop iterations; cycles decide ranking; instructions attribute work",
        "inputs":{p.name:digest(p) for pair in scripts.values() for p in pair.values()},
        "summary_median_per_iteration":summary,"records":records,
    }
    (OUT/"per-op.json").write_text(json.dumps(output,indent=2)+"\n")
    (OUT/"per-op.partial.json").unlink(missing_ok=True)
    (OUT/"splay-one-run-opcode-census.json").write_text(json.dumps(opcode_census(),indent=2)+"\n")
    print("\nMedian cycles per iteration / best reference")
    for case,row in summary.items():
        print(case,round(row["quench"]["cycles_elapsed"],1),row["best_reference_by_cycles"],round(row[row["best_reference_by_cycles"]]["cycles_elapsed"],1),round(row["quench_to_best_reference_cycles_ratio"],2))

if __name__=="__main__": main()
