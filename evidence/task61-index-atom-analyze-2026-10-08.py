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
by_fixture = defaultdict(list)
with paired_path.open(newline="") as stream:
    for row in csv.DictReader(stream):
        by_fixture[row["fixture"]].append(row)

bootstrap_samples = 100_000
seed = 61_102_028
summary = {
    "method": "paired bootstrap over per-round candidate-minus-baseline percentages and RSS bytes",
    "bootstrap_samples": bootstrap_samples,
    "seed": seed,
    "fixtures": {},
}

for fixture, rows in sorted(by_fixture.items()):
    instruction_deltas = [
        (int(row["candidate_instructions"]) - int(row["baseline_instructions"]))
        / int(row["baseline_instructions"])
        * 100
        for row in rows
    ]
    elapsed_deltas = [
        (float(row["candidate_elapsed_seconds"])
         - float(row["baseline_elapsed_seconds"]))
        / float(row["baseline_elapsed_seconds"])
        * 100
        for row in rows
    ]
    rss_deltas = [
        int(row["candidate_max_rss_bytes"]) - int(row["baseline_max_rss_bytes"])
        for row in rows
    ]
    rng = random.Random(seed)

    def median_interval(values):
        samples = [
            statistics.median(values[rng.randrange(len(values))] for _ in values)
            for _ in range(bootstrap_samples)
        ]
        samples.sort()
        return samples[2_499], samples[97_499]

    summary["fixtures"][fixture] = {
        "pairs": len(rows),
        "baseline_instruction_median": statistics.median(
            int(row["baseline_instructions"]) for row in rows
        ),
        "candidate_instruction_median": statistics.median(
            int(row["candidate_instructions"]) for row in rows
        ),
        "instruction_delta_median_percent": statistics.median(instruction_deltas),
        "instruction_delta_95_percentile_interval": median_interval(instruction_deltas),
        "candidate_lower_instruction_pairs": sum(delta < 0 for delta in instruction_deltas),
        "baseline_elapsed_median_seconds": statistics.median(
            float(row["baseline_elapsed_seconds"]) for row in rows
        ),
        "candidate_elapsed_median_seconds": statistics.median(
            float(row["candidate_elapsed_seconds"]) for row in rows
        ),
        "elapsed_delta_median_percent": statistics.median(elapsed_deltas),
        "elapsed_delta_95_percentile_interval": median_interval(elapsed_deltas),
        "candidate_faster_elapsed_pairs": sum(delta < 0 for delta in elapsed_deltas),
        "baseline_max_rss_median_bytes": statistics.median(
            int(row["baseline_max_rss_bytes"]) for row in rows
        ),
        "candidate_max_rss_median_bytes": statistics.median(
            int(row["candidate_max_rss_bytes"]) for row in rows
        ),
        "max_rss_delta_median_bytes": statistics.median(rss_deltas),
        "max_rss_delta_95_percentile_interval_bytes": median_interval(rss_deltas),
        "candidate_lower_rss_pairs": sum(delta < 0 for delta in rss_deltas),
    }

with summary_path.open("x") as stream:
    json.dump(summary, stream, indent=2)
    stream.write("\n")
