#!/usr/bin/env python3
import csv
import json
import random
import statistics
import sys
from collections import defaultdict
from pathlib import Path

if len(sys.argv) != 3:
    raise SystemExit("usage: analyze.py PAIRED.csv SUMMARY.json")

paired_path, summary_path = map(Path, sys.argv[1:])
rows = defaultdict(list)
with paired_path.open(newline="") as stream:
    for row in csv.DictReader(stream):
        rows[row["case"]].append(row)

bootstrap_samples = 100_000
seed = 61_102_027
summary = {
    "method": "paired bootstrap over per-round candidate-minus-baseline percentages",
    "bootstrap_samples": bootstrap_samples,
    "seed": seed,
    "cases": {},
}

for case, samples in sorted(rows.items()):
    instruction_deltas = [
        (int(row["candidate_instructions"]) - int(row["baseline_instructions"]))
        / int(row["baseline_instructions"])
        * 100
        for row in samples
    ]
    elapsed_deltas = [
        (float(row["candidate_elapsed_seconds"])
         - float(row["baseline_elapsed_seconds"]))
        / float(row["baseline_elapsed_seconds"])
        * 100
        for row in samples
    ]
    rng = random.Random(seed)
    bootstrapped_medians = [
        statistics.median(
            instruction_deltas[rng.randrange(len(instruction_deltas))]
            for _ in instruction_deltas
        )
        for _ in range(bootstrap_samples)
    ]
    bootstrapped_medians.sort()
    summary["cases"][case] = {
        "pairs": len(samples),
        "baseline_instruction_median": statistics.median(
            int(row["baseline_instructions"]) for row in samples
        ),
        "candidate_instruction_median": statistics.median(
            int(row["candidate_instructions"]) for row in samples
        ),
        "instruction_delta_median_percent": statistics.median(instruction_deltas),
        "instruction_delta_95_percentile_interval": [
            bootstrapped_medians[2_499],
            bootstrapped_medians[97_499],
        ],
        "candidate_lower_instruction_pairs": sum(
            delta < 0 for delta in instruction_deltas
        ),
        "elapsed_delta_median_percent": statistics.median(elapsed_deltas),
        "candidate_faster_elapsed_pairs": sum(delta < 0 for delta in elapsed_deltas),
    }

with summary_path.open("x") as stream:
    json.dump(summary, stream, indent=2)
    stream.write("\n")
