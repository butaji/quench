# v8run.py rounds "ENV=.. ENV=.." ... : median Score and median peak RSS per fixture per pacing setting
import os, statistics, subprocess, sys
rounds = int(sys.argv[1])
fixtures = ["crypto","deltablue","earley-boyer","navier-stokes","raytrace","regexp","richards","splay"]
binary = "/home/user/quench/target/gct/production/quench-node"
for cfg in sys.argv[2:]:
    env = dict(os.environ)
    env.update(kv.split("=", 1) for kv in cfg.split())
    for f in fixtures:
        scores, peaks = [], []
        for _ in range(rounds):
            proc = subprocess.Popen([binary, f"/tmp/claude-0/s/v8/{f}.js"], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            out = proc.stdout.read()
            _, _, usage = os.wait4(proc.pid, 0)
            proc.returncode = 0
            scores.append(next((float(l.split()[1]) for l in out.splitlines() if l.startswith("Score: ")), float("nan")))
            peaks.append(usage.ru_maxrss // 1024)
        print(f"[{cfg}] {f} score={statistics.median(scores):.0f} rssMB={statistics.median(peaks)}", flush=True)
