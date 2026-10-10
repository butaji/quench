#!/usr/bin/env python3
"""11 alternating production pairs for the function-source cache on EarleyBoyer."""
import csv, hashlib, re, subprocess
from pathlib import Path

ROOT = Path.cwd()
BASELINE = ROOT / "target/iteration/task61-descriptor-transition-20261009/production/quench-node"
CANDIDATE = ROOT / "target/iteration/task61-function-source-cache-production-20261009/production/quench-node"
FIXTURE = ROOT / "target/iteration/task61-argument-shapes-profile-20261009/materialized-earley-boyer.js"
OUTPUT = ROOT / "tasks/evidence/task61-function-source-cache-earley-pairs-2026-10-09.csv"
EXPECTED = {
    BASELINE: "eabe4cb354506920ef9ef217bcc935403ec49b2d61748b8ff476b0efde334d28",
    CANDIDATE: "cf3916764bd3b59ac3dcb91bd9787cb31ec01537245eaa785ae1cd9e7d6d1d57",
    FIXTURE: "aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b",
}
EXPECTED_DIFF = "2098220c345f5a6df8db0827e207c59237389741c60c55263366ae0c59ad4f27"
INSTRUCTIONS = re.compile(r"^\s*(\d+)\s+instructions retired\s*$", re.M)
RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.M)
ELAPSED = re.compile(r"^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$", re.M)
SCORE = re.compile(r"^Score:\s*(-?[\d.]+)\s*$", re.M)
BENCH = re.compile(r"^__quenchBenchResult: (.*?): (-?[\d.]+)\s*$", re.M)
sha = lambda data: hashlib.sha256(data).hexdigest()

def normalize(stdout: str) -> str:
    text = BENCH.sub(r"__quenchBenchResult: \1: <score>", stdout)
    return "\n".join(line for line in text.splitlines() if line.strip() != "----" and not line.startswith("Score:"))

for path, expected in EXPECTED.items():
    actual = sha(path.read_bytes())
    if actual != expected:
        raise SystemExit(f"hash mismatch for {path}: expected {expected}, got {actual}")
diff = subprocess.run(
    [
        "git",
        "diff",
        "--",
        "crates/quench-runtime/src/vm/construction.rs",
        "crates/quench-runtime/src/vm/function.rs",
        "crates/quench-runtime/src/vm/program_store.rs",
        "crates/quench-runtime/src/vm/tests.rs",
    ],
    cwd=ROOT,
    check=True,
    capture_output=True,
).stdout
diff_sha = sha(diff)
if diff_sha != EXPECTED_DIFF:
    raise SystemExit(f"source diff changed: expected {EXPECTED_DIFF}, got {diff_sha}")
rows = []
for pair in range(1, 12):
    order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
    measured = {}
    for label in order:
        binary = CANDIDATE if label == "candidate" else BASELINE
        result = subprocess.run(["/usr/bin/time", "-l", str(binary), str(FIXTURE)], text=True, capture_output=True)
        match_i, match_r, match_t, match_s = INSTRUCTIONS.search(result.stderr), RSS.search(result.stderr), ELAPSED.search(result.stderr), SCORE.search(result.stdout)
        if not all((match_i, match_r, match_t, match_s)):
            raise SystemExit(f"missing measurement pair={pair} label={label} exit={result.returncode}: stdout={result.stdout!r} stderr={result.stderr!r}")
        elapsed, user, system = map(float, match_t.groups())
        normalized = normalize(result.stdout)
        measured[label] = {
            "exit": result.returncode, "score": float(match_s.group(1)),
            "instructions": int(match_i.group(1)), "max_rss_bytes": int(match_r.group(1)),
            "elapsed_seconds": elapsed, "user_seconds": user, "system_seconds": system,
            "stdout": result.stdout, "stdout_sha256": sha(result.stdout.encode()),
            "normalized_stdout": normalized, "normalized_stdout_sha256": sha(normalized.encode()),
            "time_stderr": result.stderr,
        }
        if result.returncode != 0:
            raise SystemExit(f"nonzero exit pair={pair} label={label}: {result.returncode}")
    row = {"pair": pair, "order": ">".join(order), "baseline_binary_sha256": EXPECTED[BASELINE], "candidate_binary_sha256": EXPECTED[CANDIDATE], "fixture_sha256": EXPECTED[FIXTURE], "source_diff_sha256": diff_sha}
    for label in ("baseline", "candidate"):
        for field in ("score", "instructions", "max_rss_bytes", "elapsed_seconds", "user_seconds", "system_seconds", "exit", "stdout_sha256", "normalized_stdout_sha256", "stdout", "normalized_stdout", "time_stderr"):
            row[f"{label}_{field}"] = measured[label][field]
    row["normalized_outputs_match"] = measured["baseline"]["normalized_stdout"] == measured["candidate"]["normalized_stdout"]
    rows.append(row)
    print(f"pair {pair}/11 complete", flush=True)
fields = ["pair", "order", "baseline_binary_sha256", "candidate_binary_sha256", "fixture_sha256", "source_diff_sha256"]
for label in ("baseline", "candidate"):
    fields += [f"{label}_{field}" for field in ("score", "instructions", "max_rss_bytes", "elapsed_seconds", "user_seconds", "system_seconds", "exit", "stdout_sha256", "normalized_stdout_sha256", "stdout", "normalized_stdout", "time_stderr")]
fields.append("normalized_outputs_match")
with OUTPUT.open("w", newline="") as stream:
    writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
print(f"csv={OUTPUT}")
