#!/usr/bin/env python3
import csv
import hashlib
import pathlib
import re
import subprocess
import sys

if len(sys.argv) != 5:
    raise SystemExit("usage: paired.py BASELINE CANDIDATE FIXTURE_DIR OUTPUT.csv")

baseline, candidate, fixture_dir, output = map(pathlib.Path, sys.argv[1:])
CASES = (
    "dense-literal",
    "new-array-filled",
    "single-push-filled",
    "length-setter-filled",
    "map",
)
PAIRS = 11


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
    if instructions is None or elapsed is None:
        raise RuntimeError(f"time output lacks instruction/time fields:\n{report}")
    return int(instructions.group(1)), float(elapsed.group(1))


with output.open("w", newline="") as stream:
    writer = csv.writer(stream, lineterminator="\n")
    writer.writerow([
        "case", "pair", "first", "baseline_instructions", "candidate_instructions",
        "baseline_elapsed_seconds", "candidate_elapsed_seconds", "baseline_sha256",
        "candidate_sha256", "fixture_sha256",
    ])
    baseline_hash = sha256(baseline)
    candidate_hash = sha256(candidate)
    for case in CASES:
        fixture = fixture_dir / f"{case}.js"
        fixture_hash = sha256(fixture)
        for pair in range(1, PAIRS + 1):
            first, second = (candidate, baseline) if pair % 2 else (baseline, candidate)
            first_result = run(first, fixture)
            second_result = run(second, fixture)
            results = {first: first_result, second: second_result}
            writer.writerow([
                case,
                pair,
                "candidate" if first == candidate else "baseline",
                results[baseline][0],
                results[candidate][0],
                f"{results[baseline][1]:.6f}",
                f"{results[candidate][1]:.6f}",
                baseline_hash,
                candidate_hash,
                fixture_hash,
            ])
