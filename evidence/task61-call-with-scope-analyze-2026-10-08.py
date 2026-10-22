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
    deltas = [
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
    rss_deltas = [
        (int(row["candidate_maximum_rss_bytes"])
         - int(row["baseline_maximum_rss_bytes"]))
        / int(row["baseline_maximum_rss_bytes"])
        * 100
        for row in samples
    ]
    rng = random.Random(seed)
    bootstrapped_medians = [
        statistics.median(
            deltas[rng.randrange(len(deltas))] for _ in deltas
        )
        for _ in range(bootstrap_samples)
    ]
    bootstrapped_medians.sort()
    rss_rng = random.Random(seed + 1)
    bootstrapped_rss_medians = [
        statistics.median(
            rss_deltas[rss_rng.randrange(len(rss_deltas))] for _ in rss_deltas
        )
        for _ in range(bootstrap_samples)
    ]
    bootstrapped_rss_medians.sort()
    summary["cases"][case] = {
        "pairs": len(samples),
        "baseline_instruction_median": statistics.median(
            int(row["baseline_instructions"]) for row in samples
        ),
        "candidate_instruction_median": statistics.median(
            int(row["candidate_instructions"]) for row in samples
        ),
        "instruction_delta_median_percent": statistics.median(deltas),
        "instruction_delta_95_percentile_interval": [
            bootstrapped_medians[2_499],
            bootstrapped_medians[97_499],
        ],
        "candidate_lower_instruction_pairs": sum(delta < 0 for delta in deltas),
        "elapsed_delta_median_percent": statistics.median(elapsed_deltas),
        "candidate_faster_elapsed_pairs": sum(delta < 0 for delta in elapsed_deltas),
        "baseline_maximum_rss_median_bytes": statistics.median(
            int(row["baseline_maximum_rss_bytes"]) for row in samples
        ),
        "candidate_maximum_rss_median_bytes": statistics.median(
            int(row["candidate_maximum_rss_bytes"]) for row in samples
        ),
        "maximum_rss_delta_median_percent": statistics.median(rss_deltas),
        "maximum_rss_delta_95_percentile_interval": [
            bootstrapped_rss_medians[2_499],
            bootstrapped_rss_medians[97_499],
        ],
        "candidate_lower_maximum_rss_pairs": sum(delta < 0 for delta in rss_deltas),
    }

with summary_path.open("x") as stream:
    json.dump(summary, stream, indent=2)
    stream.write("\n")
