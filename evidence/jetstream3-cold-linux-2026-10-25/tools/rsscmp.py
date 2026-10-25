# usage: rsscmp.py TEST BIN... ; prints score and max RSS (KiB) per binary, one child per run
import sys, subprocess, resource, re, os
S = os.path.dirname(os.path.abspath(sys.argv[0])); test = sys.argv[1]
for b in sys.argv[2:]:
    pid = os.fork()
    if pid == 0:
        out = os.open(f"{S}/rss-{test}-{os.path.basename(b)}.txt", os.O_WRONLY | os.O_CREAT | os.O_TRUNC)
        os.dup2(out, 1); os.dup2(out, 2); os.chdir("/home/user/quench")
        argv = [b, S + "/js3-prelude.js", "quench-bench/jetstream", "--no-prefetch", "--test=" + test]
        os.environ.pop("BUN_OPTIONS", None)
        # No cached input: every runtime compiles everything it runs on every run.
        os.environ["QUENCH_COMPILE_CACHE"] = ""
        os.environ["BUN_RUNTIME_TRANSPILER_CACHE_PATH"] = "0"
        os.environ.pop("NODE_COMPILE_CACHE", None)
        os.environ["NODE_DISABLE_COMPILE_CACHE"] = "1"
        os.environ.pop("QUENCH_STARTUP_SNAPSHOT", None)
        if "bun" in b:
            os.environ["BUN_JSC_useJIT"] = "0"
        elif "node-v" in b:
            argv.insert(1, "--jitless")
        os.execv(b, argv)
    _, status, usage = os.wait4(pid, 0)
    text = open(f"{S}/rss-{test}-{os.path.basename(b)}.txt", errors="replace").read()
    m = re.search(r"^" + re.escape(test) + r"\s+Score\s+([0-9.]+)", text, re.M)
    print(f"{test:30} {os.path.basename(b):12} score={m.group(1) if m else 'FAIL':8} maxrss={usage.ru_maxrss} KiB", flush=True)
