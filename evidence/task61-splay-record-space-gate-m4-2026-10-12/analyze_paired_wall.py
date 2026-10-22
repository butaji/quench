#!/usr/bin/env python3
"""Recompute clean paired marginal wall deltas from schema-5 reports."""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPORTS = {
    "splay_aa": "splay-aa-schema5-11.json",
    "splay_ab": "splay-ab-schema5-11.json",
    "all8_ab": "all8-ab-schema5-11.json",
}
REPLICATES = 20_000
SEED = 0x61_F1_7E_D5_2026_1010
MASK = (1 << 64) - 1


def median(values):
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def paired_interval(values):
    state = SEED ^ len(values)
    bootstrapped = []
    for _ in range(REPLICATES):
        draw = []
        for _ in values:
            state ^= (state << 13) & MASK
            state &= MASK
            state ^= state >> 7
            state &= MASK
            state ^= (state << 17) & MASK
            state &= MASK
            draw.append(values[state % len(values)])
        bootstrapped.append(median(draw))
    bootstrapped.sort()
    return [bootstrapped[500], bootstrapped[19_499]]


def clean(sample):
    contention = sample.get("contention", {})
    return (
        sample.get("status") == 0
        and sample.get("timed_out") is False
        and contention.get("clean") is True
        and contention.get("checks_complete") is True
    )


def analyze(path):
    report = json.loads(path.read_text())
    fixtures = {}
    for fixture_path, fixture in report["fixtures"].items():
        values = []
        for round_record in fixture["rounds"]:
            work = round_record["samples"]
            setup = round_record["setup_only_samples"]
            names = ("quench_baseline", "quench_candidate")
            samples = [work[name] for name in names] + [setup[name] for name in names]
            if not all(clean(sample) for sample in samples):
                continue
            baseline = work["quench_baseline"]["wall_ns"] - setup["quench_baseline"]["wall_ns"]
            candidate = work["quench_candidate"]["wall_ns"] - setup["quench_candidate"]["wall_ns"]
            values.append(100 * (candidate / baseline - 1))
        fixtures[Path(fixture_path).name] = {
            "clean_pairs": len(values),
            "paired_marginal_wall_delta_median_percent": median(values) if values else None,
            "paired_marginal_wall_delta_interval_95_percent": paired_interval(values) if values else None,
        }
    return {
        "report": path.name,
        "measurement_runner_sha256": report["measurement_runner"]["sha256"],
        "baseline_sha256": next(engine["executable_sha256"] for engine in report["engines"] if engine["name"] == "quench_baseline"),
        "candidate_sha256": next(engine["executable_sha256"] for engine in report["engines"] if engine["name"] == "quench_candidate"),
        "bootstrap": {
            "statistic": "median of paired candidate-vs-baseline percent deltas",
            "replicates": REPLICATES,
            "percentile_interval": 95,
            "seed": f"0x{SEED:016x} XOR clean_pair_count",
        },
        "fixtures": fixtures,
    }


print(json.dumps({key: analyze(HERE / filename) for key, filename in REPORTS.items()}, indent=2))
