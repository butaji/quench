#!/usr/bin/env python3
"""Derive the direct-object Splay candidate's schema-5 M4 gate."""

import hashlib
import json
import random
import statistics
from pathlib import Path


HERE = Path(__file__).parent
AA_REPORT = "all-eight-aa-schema5-11.json"
AB_REPORT = "all-eight-direct-object-classification-vs-trunk-11.json"
PARENT_REPORT = "splay-direct-object-classification-11.json"
RESAMPLES = 100_000
LOWER_INDEX = 2_499
UPPER_INDEX = 97_499
SEED = 61_101_013
BASELINE = "quench_baseline"
CANDIDATE = "quench_candidate"
METRICS = ("wall_ns", "cycles", "instructions", "peak_rss_bytes")


def read_report(name):
    return json.loads((HERE / name).read_text())


def sha256(name):
    return hashlib.sha256((HERE / name).read_bytes()).hexdigest()


def is_clean(sample):
    return sample["status"] == 0 and sample["contention"].get("clean") is True


def paired_deltas(report, fixture, metric):
    record = report["fixtures"][fixture]
    run_count = (
        record["plan"]["benchmark_count"]
        * record["plan"]["iterations_per_benchmark"]
    )
    result = []
    for round_record in record["rounds"]:
        base = round_record["samples"][BASELINE]
        candidate = round_record["samples"][CANDIDATE]
        base_setup = round_record["setup_only_samples"][BASELINE]
        candidate_setup = round_record["setup_only_samples"][CANDIDATE]
        if not all(is_clean(sample) for sample in (base, candidate, base_setup, candidate_setup)):
            continue
        if metric == "peak_rss_bytes":
            result.append(100 * (candidate[metric] / base[metric] - 1))
        else:
            base_work = (base[metric] - base_setup[metric]) / run_count
            candidate_work = (candidate[metric] - candidate_setup[metric]) / run_count
            result.append(100 * (candidate_work / base_work - 1))
    return result


def bootstrap_interval(values, seed):
    rng = random.Random(seed)
    count = len(values)
    medians = sorted(
        statistics.median(values[rng.randrange(count)] for _ in range(count))
        for _ in range(RESAMPLES)
    )
    return [medians[LOWER_INDEX], medians[UPPER_INDEX]]


def summarize(values, seed):
    return {
        "clean_pairs": len(values),
        "median_delta_percent": statistics.median(values),
        "paired_bootstrap_95_percentile_interval_percent": bootstrap_interval(values, seed),
    }


def main():
    aa = read_report(AA_REPORT)
    ab = read_report(AB_REPORT)
    parent = read_report(PARENT_REPORT)
    fixtures = {}
    for fixture_index, key in enumerate(sorted(ab["fixtures"])):
        fixture_name = key.rsplit("/", 1)[-1].removesuffix(".js")
        aa_rows = {}
        ab_rows = {}
        for metric_index, metric in enumerate(METRICS):
            seed = SEED + fixture_index * len(METRICS) + metric_index
            aa_rows[metric] = summarize(paired_deltas(aa, key, metric), seed)
            ab_rows[metric] = summarize(
                paired_deltas(ab, key, metric), seed + 100
            )
        allowances = {
            "wall_percent": aa_rows["wall_ns"][
                "paired_bootstrap_95_percentile_interval_percent"
            ][1],
            "rss_percent": aa_rows["peak_rss_bytes"][
                "paired_bootstrap_95_percentile_interval_percent"
            ][1],
        }
        fixtures[fixture_name] = {
            "aa": aa_rows,
            "ab": ab_rows,
            "aa_adjusted_point_losses": {
                "wall_percent": max(
                    0,
                    ab_rows["wall_ns"]["median_delta_percent"]
                    - allowances["wall_percent"],
                ),
                "rss_percent": max(
                    0,
                    ab_rows["peak_rss_bytes"]["median_delta_percent"]
                    - allowances["rss_percent"],
                ),
            },
        }

    aa_clean = all(
        row["aa"]["wall_ns"]["clean_pairs"] == 11
        and row["aa"]["peak_rss_bytes"]["clean_pairs"] == 11
        for row in fixtures.values()
    )
    ab_clean = all(
        row["ab"]["wall_ns"]["clean_pairs"] == 11
        and row["ab"]["peak_rss_bytes"]["clean_pairs"] == 11
        and ab["fixtures"][key]["output_equal"]
        for key, row in zip(sorted(ab["fixtures"]), fixtures.values())
    )
    non_splay_pass = all(
        row["aa_adjusted_point_losses"]["wall_percent"] <= 2.0
        and row["aa_adjusted_point_losses"]["rss_percent"] <= 1.0
        for name, row in fixtures.items()
        if name != "splay"
    )
    splay = fixtures["splay"]["ab"]
    active_wall_gain = splay["wall_ns"][
        "paired_bootstrap_95_percentile_interval_percent"
    ][1] < 0
    active_rss_gain = splay["peak_rss_bytes"][
        "paired_bootstrap_95_percentile_interval_percent"
    ][1] < 0

    parent_splay = paired_deltas(
        parent, next(key for key in parent["fixtures"] if key.endswith("splay.js")), "wall_ns"
    )
    parent_instructions = paired_deltas(
        parent,
        next(key for key in parent["fixtures"] if key.endswith("splay.js")),
        "instructions",
    )
    direct_to_parent = {
        "clean_pairs": len(parent_splay),
        "wall": summarize(parent_splay, SEED + 400),
        "instructions": summarize(parent_instructions, SEED + 401),
    }

    engine_hashes = {
        report_name: {
            engine["name"]: engine["executable_sha256"]
            for engine in report["engines"]
        }
        for report_name, report in ((AA_REPORT, aa), (AB_REPORT, ab), (PARENT_REPORT, parent))
    }
    result = {
        "schema": 1,
        "platform": "M4 arm64 macOS, Mac16,10",
        "method": (
            "Schema-5 fixed-work reports; each fixture uses clean paired rows only. "
            "Wall, cycles and instructions are K=0-subtracted marginal values per run; "
            "RSS is raw work max RSS. A/A-adjusted point losses subtract the same-fixture "
            "A/A upper 95th percentile endpoint. Bootstrap uses 100,000 paired-median "
            "resamples with deterministic seeds."
        ),
        "bootstrap": {
            "resamples": RESAMPLES,
            "percentile_indices": [LOWER_INDEX, UPPER_INDEX],
            "base_seed": SEED,
        },
        "reports": {
            name: sha256(name)
            for name in (AA_REPORT, AB_REPORT, PARENT_REPORT)
        },
        "runner_sha256": ab["measurement_runner"]["sha256"],
        "corpus_revision": ab["corpus"].get("pinned_revision"),
        "engine_sha256": engine_hashes,
        "fixtures": fixtures,
        "direct_object_fast_path_vs_parent": direct_to_parent,
        "guard": {
            "aa_all_eight_11_clean": aa_clean,
            "ab_all_eight_11_clean_and_output_equal": ab_clean,
            "other_fixture_caps_after_aa_allowance": {
                "wall_percent": 2.0,
                "rss_percent": 1.0,
            },
            "other_fixture_point_screen_passes": non_splay_pass,
            "splay_wall_gain_resolved": active_wall_gain,
            "splay_rss_gain_resolved": active_rss_gain,
            "m4_fixed_work_gate_passes": (
                aa_clean and ab_clean and non_splay_pass and active_wall_gain and active_rss_gain
            ),
            "stock_harness_and_linux_gates_pending": True,
        },
    }
    (HERE / "all-eight-analysis-direct-object.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    for name, row in fixtures.items():
        wall = row["ab"]["wall_ns"]
        rss = row["ab"]["peak_rss_bytes"]
        loss = row["aa_adjusted_point_losses"]
        print(
            f"{name:18} n={wall['clean_pairs']} wall "
            f"{wall['median_delta_percent']:+.3f}% "
            f"[{wall['paired_bootstrap_95_percentile_interval_percent'][0]:+.3f}, "
            f"{wall['paired_bootstrap_95_percentile_interval_percent'][1]:+.3f}] "
            f"RSS {rss['median_delta_percent']:+.3f}% "
            f"[{rss['paired_bootstrap_95_percentile_interval_percent'][0]:+.3f}, "
            f"{rss['paired_bootstrap_95_percentile_interval_percent'][1]:+.3f}] "
            f"AA-adjusted loss {loss['wall_percent']:.3f}%/{loss['rss_percent']:.3f}%"
        )
    print("Splay fast path vs parent", json.dumps(direct_to_parent, sort_keys=True))
    print("guard", json.dumps(result["guard"], sort_keys=True))


if __name__ == "__main__":
    main()
