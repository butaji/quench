#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import os
import pathlib
import statistics
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("benchmark_runner", OUT / "benchmark-runner.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

binary_dir = ROOT / "work/task61-v2-merge-earley/loadconst-capture-binaries"
baseline = pathlib.Path(os.environ.get("TASK61_BASELINE_BINARY", binary_dir / "baseline-binary"))
candidate = pathlib.Path(os.environ.get("TASK61_CANDIDATE_BINARY", binary_dir / "candidate-binary"))
fixture = OUT / "materialized-earley-boyer.js"
oracle = OUT / "node-oracle.js"
node = "/opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node"
expected_fixture_sha256 = "aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


if sha(fixture) != expected_fixture_sha256:
    raise RuntimeError("materialized EarleyBoyer fixture hash changed")
expected = subprocess.run([node, str(oracle)], capture_output=True, text=True, check=True).stdout.splitlines()
for name, binary in (("baseline", baseline), ("candidate", candidate)):
    result = subprocess.run([str(binary), str(oracle)], capture_output=True, text=True, check=True)
    if result.stdout.splitlines() != expected:
        raise RuntimeError(f"{name} differs from the Node oracle")

baseline_score = OUT / "baseline-binary"
candidate_score = OUT / "candidate-binary"
raw = json.loads((ROOT / "tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json").read_text())
env = next(engine["environment"] for engine in raw["engines"] if engine["name"] == "quench")
rounds = int(sys.argv[1])
tag = sys.argv[2] if len(sys.argv) > 2 else str(rounds)
log = OUT / f"pairs-{tag}.jsonl"
report_path = OUT / f"paired-{tag}.json"
if log.exists() or report_path.exists():
    raise RuntimeError(f"refusing to overwrite {tag} results")
pairs = []

for pair in range(1, rounds + 1):
    order = ["baseline", "candidate"] if pair % 2 else ["candidate", "baseline"]
    result = {}
    for variant in order:
        binary = baseline_score if variant == "baseline" else candidate_score
        result[variant] = runner.run(binary, fixture, env, f"earley-loadconst-capture-{tag}-{pair:02d}-{variant}")
    before, after = result["baseline"], result["candidate"]
    equal = before["semantic_output"] == after["semantic_output"]
    row = {
        "pair": pair,
        "order": order,
        "valid": before["exit_code"] == after["exit_code"] == 0
        and before["score"] is not None
        and after["score"] is not None
        and equal,
        "output_equal": equal,
        "baseline_score": before["score"],
        "candidate_score": after["score"],
        "score_delta": after["score"] - before["score"],
        "baseline_rss_bytes": before["maximum_rss_bytes"],
        "candidate_rss_bytes": after["maximum_rss_bytes"],
        "rss_delta_bytes": after["maximum_rss_bytes"] - before["maximum_rss_bytes"],
        "baseline_wall_ns": before["wall_ns"],
        "candidate_wall_ns": after["wall_ns"],
    }
    pairs.append(row)
    with log.open("a") as output:
        output.write(json.dumps(row, separators=(",", ":")) + "\n")
        output.flush()
    print(
        f"pair {pair}/{rounds}: score {before['score']}->{after['score']} "
        f"({row['score_delta']:+g}); RSS {before['maximum_rss_bytes']}->"
        f"{after['maximum_rss_bytes']} ({row['rss_delta_bytes']:+d}); equal={equal}",
        flush=True,
    )


def interval(values, seed):
    import random

    rng = random.Random(seed)
    draws = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(100_000))
    return [draws[2500], draws[97499]]


score_deltas = [row["score_delta"] for row in pairs]
rss_deltas = [row["rss_delta_bytes"] for row in pairs]
report = {
    "experiment": "EarleyBoyer LoadConst -> LoadCapture dispatch fusion",
    "baseline_sha256": sha(baseline),
    "candidate_sha256": sha(candidate),
    "fixture_sha256": sha(fixture),
    "node_oracle_sha256": sha(oracle),
    "rounds": rounds,
    "bootstrap_replicates": 100_000,
    "baseline_score_median": statistics.median(row["baseline_score"] for row in pairs),
    "candidate_score_median": statistics.median(row["candidate_score"] for row in pairs),
    "score_delta_median": statistics.median(score_deltas),
    "score_delta_95_ci": interval(score_deltas, 0x610ecb7),
    "baseline_maximum_rss_median_bytes": statistics.median(row["baseline_rss_bytes"] for row in pairs),
    "candidate_maximum_rss_median_bytes": statistics.median(row["candidate_rss_bytes"] for row in pairs),
    "maximum_rss_delta_median_bytes": statistics.median(rss_deltas),
    "maximum_rss_delta_95_ci_bytes": interval(rss_deltas, 0x610ecb8),
    "valid": all(row["valid"] for row in pairs),
    "output_equal": all(row["output_equal"] for row in pairs),
    "pairs": pairs,
    "qualification_ready": False,
}
report_path.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({key: value for key, value in report.items() if key != "pairs"}, indent=2))
