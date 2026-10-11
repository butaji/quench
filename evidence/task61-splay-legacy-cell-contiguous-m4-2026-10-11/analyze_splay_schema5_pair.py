#!/usr/bin/env python3
"""Recompute the fixed-work Splay paired gate metrics from a schema-5 report."""
import hashlib
import json
import math
import pathlib
import sys

BOOTSTRAP_REPLICATES = 20_000
BOOTSTRAP_SEED = 0x61_F1_7E_D5_2026_1010
BASELINE = "quench_baseline"
CANDIDATE = "quench_candidate"
METRICS = ("wall_ns", "cycles", "instructions")
MASK = (1 << 64) - 1


def median(values):
    values = sorted(values)
    middle = len(values) // 2
    if len(values) % 2:
        return values[middle]
    return (values[middle - 1] + values[middle]) / 2


def interval(values):
    state = BOOTSTRAP_SEED ^ len(values)
    estimates = []
    for _ in range(BOOTSTRAP_REPLICATES):
        draw = []
        for _ in values:
            state ^= state << 13 & MASK
            state &= MASK
            state ^= state >> 7
            state &= MASK
            state ^= state << 17 & MASK
            state &= MASK
            draw.append(values[state % len(values)])
        estimates.append(median(draw))
    estimates.sort()
    lower = math.floor(0.025 * (BOOTSTRAP_REPLICATES - 1) + 0.5)
    upper = math.floor(0.975 * (BOOTSTRAP_REPLICATES - 1) + 0.5)
    return [estimates[lower], estimates[upper]]


def main(path):
    path = pathlib.Path(path)
    data = path.read_bytes()
    report = json.loads(data)
    assert report["schema"] == 5
    assert report["measurement_mode"] == "fixed_work_diagnostic"
    assert report["rounds_requested"] == 11
    assert len(report["engines"]) == 2
    fixture = next(value for key, value in report["fixtures"].items() if key.endswith("/splay.js"))
    assert fixture["valid"] and fixture["output_equal"]
    assert fixture["plan"]["iterations_per_benchmark"] == 300
    assert len(fixture["rounds"]) == 11

    deltas = {metric: [] for metric in METRICS}
    rss = []
    for expected_round, round_data in enumerate(fixture["rounds"]):
        assert round_data["round"] == expected_round
        samples = round_data["samples"]
        setup = round_data["setup_only_samples"]
        bw, bs = samples[BASELINE], setup[BASELINE]
        cw, cs = samples[CANDIDATE], setup[CANDIDATE]
        assert all(s["contention"]["clean"] for s in (bw, bs, cw, cs))
        for metric in METRICS:
            baseline = bw[metric] - bs[metric]
            candidate = cw[metric] - cs[metric]
            deltas[metric].append(100.0 * (candidate / baseline - 1.0))
        rss.append(100.0 * (cw["peak_rss_bytes"] / bw["peak_rss_bytes"] - 1.0))

    summaries = fixture["summaries"]
    result = {
        "schema": 1,
        "measurement": "paired fixed-work Splay; 300 run() calls; K=0 differenced",
        "report_sha256": hashlib.sha256(data).hexdigest(),
        "source_revision": report["source_revision"],
        "source_dirty": report["source_dirty"],
        "platform": report["host"]["uname"],
        "logical_cpus": report["host"]["logical_cpus"],
        "binary_sha256": {
            engine["name"]: engine["executable_sha256"] for engine in report["engines"]
        },
        "valid_rounds": len(fixture["rounds"]),
        "clean_pairs": len(rss),
        "marginal_per_run": {
            engine: summaries[engine]["marginal_per_run"] for engine in (BASELINE, CANDIDATE)
        },
        "paired_candidate_vs_baseline_percent": {
            metric: {"median": median(values), "bootstrap_95": interval(values)}
            for metric, values in deltas.items()
        },
        "work_max_rss_candidate_vs_baseline_percent": {
            "median": median(rss),
            "bootstrap_95": interval(rss),
            "baseline_median_bytes": summaries[BASELINE]["work"]["median_max_rss_bytes"],
            "candidate_median_bytes": summaries[CANDIDATE]["work"]["median_max_rss_bytes"],
        },
    }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} FIXED_WORK_REPORT.json")
    main(sys.argv[1])
