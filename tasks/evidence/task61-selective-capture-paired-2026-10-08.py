#!/usr/bin/env python3
import csv
import hashlib
import pathlib
import re
import subprocess
import sys

if len(sys.argv) != 5:
    raise SystemExit("usage: paired.py CANDIDATE BASELINE FIXTURE OUTPUT.csv")

candidate, baseline, fixture, output = map(pathlib.Path, sys.argv[1:])

def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def run(binary):
    result = subprocess.run(
        ["/usr/bin/time", "-l", str(binary), str(fixture)],
        check=True,
        capture_output=True,
        text=True,
    )
    report = result.stderr + result.stdout
    retired = re.search(r"(?m)^\s*(\d+)\s+instructions retired\s*$", report)
    elapsed = re.search(r"(?m)^\s*([0-9.]+)\s+real\s+", report)
    if retired is None or elapsed is None:
        raise RuntimeError(f"time output lacks instruction/time fields:\n{report}")
    return int(retired.group(1)), float(elapsed.group(1))

with output.open("w", newline="") as stream:
    writer = csv.writer(stream)
    writer.writerow([
        "pair", "first", "candidate_instructions", "baseline_instructions",
        "candidate_elapsed_seconds", "baseline_elapsed_seconds",
        "candidate_sha256", "baseline_sha256", "fixture_sha256",
    ])
    candidate_hash = sha256(candidate)
    baseline_hash = sha256(baseline)
    fixture_hash = sha256(fixture)
    for pair in range(1, 12):
        first, second = (candidate, baseline) if pair % 2 else (baseline, candidate)
        first_result = run(first)
        second_result = run(second)
        results = {first: first_result, second: second_result}
        candidate_result = results[candidate]
        baseline_result = results[baseline]
        writer.writerow([
            pair,
            "candidate" if first == candidate else "baseline",
            candidate_result[0],
            baseline_result[0],
            f"{candidate_result[1]:.6f}",
            f"{baseline_result[1]:.6f}",
            candidate_hash,
            baseline_hash,
            fixture_hash,
        ])
