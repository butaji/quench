#!/usr/bin/env python3
import csv
import hashlib
import pathlib
import re
import subprocess
import sys

if len(sys.argv) != 4:
    raise SystemExit("usage: paired.py BASELINE CANDIDATE OUTPUT.csv")

baseline, candidate, output = map(pathlib.Path, sys.argv[1:])
fixture_dir = pathlib.Path(__file__).with_name("task61-index-atom-fixtures")
fixtures = ("fill-1m.js", "two-arrays-500k.js", "dense-overwrite-1m.js")
pair_count = 11


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(binary, fixture):
    result = subprocess.run(
        ["/usr/bin/time", "-l", str(binary), str(fixture)],
        check=True,
        capture_output=True,
        text=True,
    )
    report = result.stderr + result.stdout
    instructions = re.search(r"(?m)^\s*(\d+)\s+instructions retired\s*$", report)
    elapsed = re.search(r"(?m)^\s*([0-9.]+)\s+real\s+", report)
    max_rss = re.search(r"(?m)^\s*(\d+)\s+maximum resident set size\s*$", report)
    if instructions is None or elapsed is None or max_rss is None:
        raise RuntimeError(f"time output lacks measurement fields:\n{report}")
    return int(instructions.group(1)), float(elapsed.group(1)), int(max_rss.group(1))


with output.open("w", newline="") as stream:
    writer = csv.writer(stream, lineterminator="\n")
    writer.writerow([
        "fixture", "pair", "first", "baseline_instructions", "candidate_instructions",
        "baseline_elapsed_seconds", "candidate_elapsed_seconds", "baseline_max_rss_bytes",
        "candidate_max_rss_bytes", "baseline_sha256", "candidate_sha256", "fixture_sha256",
    ])
    baseline_hash = sha256(baseline)
    candidate_hash = sha256(candidate)
    for name in fixtures:
        fixture = fixture_dir / name
        fixture_hash = sha256(fixture)
        for pair in range(1, pair_count + 1):
            first, second = (candidate, baseline) if pair % 2 else (baseline, candidate)
            first_result = run(first, fixture)
            second_result = run(second, fixture)
            results = {first: first_result, second: second_result}
            writer.writerow([
                name,
                pair,
                "candidate" if first == candidate else "baseline",
                results[baseline][0],
                results[candidate][0],
                f"{results[baseline][1]:.6f}",
                f"{results[candidate][1]:.6f}",
                results[baseline][2],
                results[candidate][2],
                baseline_hash,
                candidate_hash,
                fixture_hash,
            ])
