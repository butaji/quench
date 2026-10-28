#!/usr/bin/env python3
"""Compare schema-5 wall and RSS deltas with the same-binary A/A band."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
AA_PATH = HERE / "all-eight-aa-11.json"
AB_PATH = HERE / "all-eight-ab-confirm-11.json"
BOOTSTRAP_REPLICATES = 20_000
BOOTSTRAP_SEED = 0x61_F1_7E_D5_2026_1010
UINT64_MASK = (1 << 64) - 1


def median(values):
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2


def paired_interval(values):
    state = BOOTSTRAP_SEED ^ len(values)
    samples = []
    for _ in range(BOOTSTRAP_REPLICATES):
        draw = []
        for _ in values:
            state ^= (state << 13) & UINT64_MASK
            state &= UINT64_MASK
            state ^= state >> 7
            state &= UINT64_MASK
            state ^= (state << 17) & UINT64_MASK
            state &= UINT64_MASK
            draw.append(values[state % len(values)])
        samples.append(median(draw))
    samples.sort()
    return [samples[500], samples[19_499]]


def clean(sample):
    contention = sample.get("contention", {})
    return (
        sample.get("status") == 0
        and sample.get("timed_out") is False
        and contention.get("clean") is True
        and contention.get("checks_complete") is True
    )


def fixture_metrics(fixture):
    wall = []
    rss = []
    for record in fixture["rounds"]:
        work = record["samples"]
        setup = record["setup_only_samples"]
        baseline = work["quench_baseline"]
        candidate = work["quench_candidate"]
        baseline_setup = setup["quench_baseline"]
        candidate_setup = setup["quench_candidate"]
        if not all(clean(sample) for sample in (baseline, candidate, baseline_setup, candidate_setup)):
            continue
        baseline_wall = baseline["wall_ns"] - baseline_setup["wall_ns"]
        candidate_wall = candidate["wall_ns"] - candidate_setup["wall_ns"]
        wall.append(100 * (candidate_wall / baseline_wall - 1))
        rss.append(100 * (candidate["peak_rss_bytes"] / baseline["peak_rss_bytes"] - 1))
    if not wall:
        return {"clean_pairs": 0, "wall_percent": None, "wall_interval_95_percent": None,
                "max_rss_percent": None, "max_rss_interval_95_percent": None}
    return {
        "clean_pairs": len(wall),
        "wall_percent": median(wall),
        "wall_interval_95_percent": paired_interval(wall),
        "max_rss_percent": median(rss),
        "max_rss_interval_95_percent": paired_interval(rss),
    }


def report_identity(path, report):
    return {
        "path": path.name,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "baseline_sha256": next(e["executable_sha256"] for e in report["engines"] if e["name"] == "quench_baseline"),
        "candidate_sha256": next(e["executable_sha256"] for e in report["engines"] if e["name"] == "quench_candidate"),
    }


def main():
    aa = json.loads(AA_PATH.read_text())
    ab = json.loads(AB_PATH.read_text())
    fixtures = {}
    for fixture_path in sorted(ab["fixtures"]):
        aa_fixture = aa["fixtures"][fixture_path]
        ab_fixture = ab["fixtures"][fixture_path]
        fixtures[Path(fixture_path).name] = {
            "aa": fixture_metrics(aa_fixture),
            "ab": fixture_metrics(ab_fixture),
        }
    result = {
        "schema": 1,
        "method": {
            "work": "per-round marginal wall = work wall minus same-engine setup-only wall",
            "summary": "median of paired candidate-vs-baseline percentage deltas",
            "interval": "95% paired percentile bootstrap",
            "bootstrap_replicates": BOOTSTRAP_REPLICATES,
            "bootstrap_seed": f"0x{BOOTSTRAP_SEED:016x} XOR clean_pair_count",
            "clean_pairs_only": True,
        },
        "host": ab["host"]["uname"],
        "aa": report_identity(AA_PATH, aa),
        "ab": report_identity(AB_PATH, ab),
        "fixtures": fixtures,
    }
    output = HERE / "all-eight-wall-aa-ab-summary.json"
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(output)
    print(json.dumps(result["fixtures"], indent=2))


if __name__ == "__main__":
    main()
