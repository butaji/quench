#!/usr/bin/env python3
"""Edit-and-test latency for one Quench test under one build mode.
Usage: buildbench.py MODE. Each mode: one untimed warm build, then 3 timed rounds of
(edit plain_data_equal's body -> run one lib test). Reports wall seconds, peak RSS of the
largest child (KiB) and the test outcome."""
import os, resource, subprocess, sys, time, re, threading
ROOT = "/home/user/qbench"
FILE = ROOT + "/crates/quench-runtime/src/vm/plain_equality.rs"
ANCHOR = "    pub(crate) fn plain_data_equal(&self, left: Value, right: Value) -> bool {\n"
TEST = "vm::tests::plain_data_equality_proves_only_plain_data"
mode = sys.argv[1]
env = dict(os.environ)
env["CARGO_TARGET_DIR"] = f"{ROOT}/target-{mode}"
env["CARGO_BUILD_JOBS"] = "4"
base = ["test", "-q", "-p", "quench-runtime", "--lib", "--", "--exact", TEST]
if mode == "cargo-incr":
    cmd = ["cargo", "+1.99.0"] + base
elif mode == "cargo-noincr":
    env["CARGO_INCREMENTAL"] = "0"; cmd = ["cargo", "+1.99.0"] + base
elif mode == "cranelift":
    env["CARGO_PROFILE_DEV_CODEGEN_BACKEND"] = "cranelift"
    # The repo's build.rustflags carry an LLVM-only alignment flag; Cranelift rejects it.
    env["RUSTFLAGS"] = "-D warnings"
    cmd = ["cargo", "+nightly", "-Zcodegen-backend"] + base
elif mode == "rajit":
    cmd = ["cargo", "+1.99.0", "ra-jit"] + base
else:
    sys.exit("unknown mode")
original = open(FILE).read()
assert original.count(ANCHOR) == 1
def edit(n):
    open(FILE, "w").write(original.replace(ANCHOR, ANCHOR + f"        std::hint::black_box({n}u32);\n"))
peak_daemon = [0]
def watch(stop):
    while not stop.is_set():
        out = subprocess.run(["ps", "-eo", "rss,comm"], capture_output=True, text=True).stdout
        total = sum(int(l.split()[0]) for l in out.splitlines()[1:] if "ra-jit" in l)
        peak_daemon[0] = max(peak_daemon[0], total)
        time.sleep(0.5)
def run(label):
    t = time.time()
    p = subprocess.run(cmd, cwd=ROOT, env=env, capture_output=True, text=True)
    dt = time.time() - t
    ok = "test result: ok. 1 passed" in p.stdout
    tail = (p.stdout + p.stderr).strip().splitlines()[-3:]
    print(f"{mode} {label}: {dt:.1f}s ok={ok} maxrss_child_kib={resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss}", flush=True)
    if not ok: print("   ", " | ".join(tail), flush=True)
    return dt, ok
stop = threading.Event(); threading.Thread(target=watch, args=(stop,), daemon=True).start()
try:
    run("warm (full build)")
    if mode == "rajit":
        for _ in range(240):
            s = subprocess.run(["cargo", "+1.99.0", "ra-jit", "status"], cwd=ROOT, capture_output=True, text=True).stdout
            if "ready" in s: break
            time.sleep(5)
        print("rajit status:", " ".join(s.split())[:300], flush=True)
    times = []
    for n in (1, 2, 3):
        edit(n); dt, ok = run(f"edit {n}"); times.append(dt)
    print(f"{mode} SUMMARY edits={['%.1f' % t for t in times]} median={sorted(times)[1]:.1f}s daemon_peak_kib={peak_daemon[0]}", flush=True)
    if mode == "rajit":
        print(subprocess.run(["cargo", "+1.99.0", "ra-jit", "status"], cwd=ROOT, capture_output=True, text=True).stdout, flush=True)
finally:
    open(FILE, "w").write(original); stop.set()
