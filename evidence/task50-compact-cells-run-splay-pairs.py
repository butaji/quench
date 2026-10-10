import hashlib
import json
import os
import pathlib
import random
import statistics
import subprocess
import time

root = pathlib.Path("/workspace/quench")
work = root / "target/stageb-candidates/compact-cells"
fixture = root / "work/stageb-candidates/splay-memory-profile.js"
binaries = {
    "baseline": work / "baseline-quench-node",
    "candidate": work / "candidate-boxed-slice-quench-node",
}
node = pathlib.Path("/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node")
env = {key: value for key, value in os.environ.items() if key in {"PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "TZ"}}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def semantic(text):
    return "\n".join(
        line for line in text.splitlines()
        if not line.startswith(("Score: ", "__quenchBenchResult: ")) and line != "----"
    )


def run(label, ordinal):
    stdout_path = work / f"boxed-slice-splay-{ordinal:02d}-{label}.stdout"
    stderr_path = work / f"boxed-slice-splay-{ordinal:02d}-{label}.stderr"
    started = time.perf_counter_ns()
    pid = os.fork()
    if pid == 0:
        with stdout_path.open("wb") as stdout, stderr_path.open("wb") as stderr:
            os.dup2(stdout.fileno(), 1)
            os.dup2(stderr.fileno(), 2)
            os.execve(str(binaries[label]), [str(binaries[label]), str(fixture)], env)
    waited, status, usage = os.wait4(pid, 0)
    assert waited == pid
    text = stdout_path.read_text(errors="replace")
    scores = [line.removeprefix("Score: ") for line in text.splitlines() if line.startswith("Score: ")]
    return {
        "status": os.waitstatus_to_exitcode(status),
        "elapsed_ns": time.perf_counter_ns() - started,
        "score": float(scores[-1]) if len(scores) == 1 else None,
        "max_rss_bytes": usage.ru_maxrss * 1024,
        "stdout_sha256": sha(stdout_path),
        "stderr_sha256": sha(stderr_path),
        "semantic_sha256": hashlib.sha256(semantic(text).encode()).hexdigest(),
        "semantic": semantic(text),
    }


rows = []
for pair in range(11):
    order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
    samples = {label: run(label, pair * 2 + index) for index, label in enumerate(order)}
    assert all(sample["status"] == 0 and sample["score"] is not None for sample in samples.values())
    assert samples["baseline"]["semantic"] == samples["candidate"]["semantic"]
    rows.append({
        "pair": pair + 1,
        "order": order,
        **{label: {key: value for key, value in sample.items() if key != "semantic"}
           for label, sample in samples.items()},
    })
    (work / "boxed-slice-splay-paired-progress.json").write_text(json.dumps(rows, indent=2) + "\n")
    print(f"pair {pair + 1}/11: baseline={samples['baseline']['score']} candidate={samples['candidate']['score']} rss_delta={samples['candidate']['max_rss_bytes']-samples['baseline']['max_rss_bytes']}", flush=True)

node_result = subprocess.run([str(node), str(fixture)], env=env, capture_output=True, text=True, check=True)
node_semantic = semantic(node_result.stdout)
all_node_equal = all(
    semantic(path.read_text(errors="replace")) == node_semantic
    for label in ("baseline", "candidate")
    for path in work.glob(f"splay-*-{label}.stdout")
)

def bootstrap(values, seed):
    rng = random.Random(seed)
    samples = sorted(statistics.median(values[rng.randrange(len(values))] for _ in values) for _ in range(100_000))
    return [samples[2499], samples[97499]]

base_scores = [row["baseline"]["score"] for row in rows]
candidate_scores = [row["candidate"]["score"] for row in rows]
base_rss = [row["baseline"]["max_rss_bytes"] for row in rows]
candidate_rss = [row["candidate"]["max_rss_bytes"] for row in rows]
score_delta = [candidate - baseline for baseline, candidate in zip(base_scores, candidate_scores)]
rss_delta = [candidate - baseline for baseline, candidate in zip(base_rss, candidate_rss)]
report = {
    "schema": 1,
    "task": "50 / 61",
    "experiment": "compact-cell boxed-slice Splay paired screen",
    "fixture_sha256": sha(fixture),
    "candidate_binary_sha256": sha(binaries["candidate"]),
    "baseline_binary_sha256": sha(binaries["baseline"]),
    "node_version": subprocess.check_output([str(node), "--version"], text=True).strip(),
    "node_output_equal_all_runs": all_node_equal,
    "pairs": rows,
    "summary": {
        "baseline_score_median": statistics.median(base_scores),
        "candidate_score_median": statistics.median(candidate_scores),
        "score_delta_median": statistics.median(score_delta),
        "score_delta_95_ci": bootstrap(score_delta, 6101001),
        "baseline_rss_median_bytes": statistics.median(base_rss),
        "candidate_rss_median_bytes": statistics.median(candidate_rss),
        "rss_delta_median_bytes": statistics.median(rss_delta),
        "rss_delta_95_ci_bytes": bootstrap(rss_delta, 6101002),
        "all_outputs_equal": all(row["baseline"]["semantic_sha256"] == row["candidate"]["semantic_sha256"] for row in rows),
    },
}
(work / "boxed-slice-splay-paired.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report["summary"], sort_keys=True))
