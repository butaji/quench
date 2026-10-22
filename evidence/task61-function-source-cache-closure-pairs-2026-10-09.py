#!/usr/bin/env python3
"""Alternating production A/B pairs for per-program function source caching."""
import csv, hashlib, re, subprocess
from pathlib import Path

ROOT = Path.cwd()
BASELINE = ROOT / "target/iteration/task61-descriptor-transition-20261009/production/quench-node"
CANDIDATE = ROOT / "target/iteration/task61-function-source-cache-production-20261009/production/quench-node"
FIXTURE = ROOT / "tasks/evidence/task61-normal-closure-10m-2026-10-09.cjs"
OUTPUT = ROOT / "tasks/evidence/task61-function-source-cache-closure-pairs-2026-10-09.csv"
EXPECTED = {
    BASELINE: "eabe4cb354506920ef9ef217bcc935403ec49b2d61748b8ff476b0efde334d28",
    CANDIDATE: "cf3916764bd3b59ac3dcb91bd9787cb31ec01537245eaa785ae1cd9e7d6d1d57",
    FIXTURE: "08ef4a8476b48cfbc784a53d205769adf3374e8836ad302d36d251b8bad8d109",
}
INSTRUCTIONS = re.compile(r"^\s*(\d+)\s+instructions retired\s*$", re.M)
RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.M)
ELAPSED = re.compile(r"^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$", re.M)
sha = lambda data: hashlib.sha256(data).hexdigest()

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
if diff_sha != "2098220c345f5a6df8db0827e207c59237389741c60c55263366ae0c59ad4f27":
    raise SystemExit(f"source diff changed: expected 2098220c…, got {diff_sha}")
rows = []
for pair in range(1, 12):
    order = ("candidate", "baseline") if pair % 2 else ("baseline", "candidate")
    measured = {}
    for label in order:
        binary = CANDIDATE if label == "candidate" else BASELINE
        result = subprocess.run(["/usr/bin/time", "-l", str(binary), str(FIXTURE)], text=True, capture_output=True)
        instruction_match = INSTRUCTIONS.search(result.stderr)
        rss_match = RSS.search(result.stderr)
        elapsed_match = ELAPSED.search(result.stderr)
        if not all((instruction_match, rss_match, elapsed_match)):
            raise SystemExit(f"missing time output pair={pair} label={label}, exit={result.returncode}: {result.stderr}")
        elapsed, user, system = map(float, elapsed_match.groups())
        measured[label] = {
            "instructions": int(instruction_match.group(1)),
            "max_rss_bytes": int(rss_match.group(1)),
            "elapsed_seconds": elapsed, "user_seconds": user, "system_seconds": system,
            "exit": result.returncode, "stdout": result.stdout,
            "stdout_bytes": len(result.stdout.encode()), "stdout_sha256": sha(result.stdout.encode()),
        }
        if result.returncode != 0:
            raise SystemExit(f"nonzero exit pair={pair} label={label}: {result.returncode} {result.stderr}")
    row = {"pair": pair, "order": ">".join(order), "baseline_binary_sha256": EXPECTED[BASELINE], "candidate_binary_sha256": EXPECTED[CANDIDATE], "fixture_sha256": EXPECTED[FIXTURE], "source_diff_sha256": diff_sha}
    for label in ("baseline", "candidate"):
        for field in ("instructions", "max_rss_bytes", "elapsed_seconds", "user_seconds", "system_seconds", "exit", "stdout_bytes", "stdout_sha256", "stdout"):
            row[f"{label}_{field}"] = measured[label][field]
    row["outputs_match"] = measured["baseline"]["stdout"] == measured["candidate"]["stdout"]
    rows.append(row)
    print(f"pair {pair}/11 complete", flush=True)
fields = ["pair", "order", "baseline_binary_sha256", "candidate_binary_sha256", "fixture_sha256", "source_diff_sha256"]
for label in ("baseline", "candidate"):
    fields += [f"{label}_{field}" for field in ("instructions", "max_rss_bytes", "elapsed_seconds", "user_seconds", "system_seconds", "exit", "stdout_bytes", "stdout_sha256", "stdout")]
fields.append("outputs_match")
with OUTPUT.open("w", newline="") as stream:
    writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
print(f"csv={OUTPUT}")
