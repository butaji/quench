#!/usr/bin/env python3
import json
import pathlib
import random
import statistics

OUT = pathlib.Path(__file__).resolve().parent
reports = [
    json.loads((OUT / name).read_text())
    for name in (
        "paired-confirm.json",
        "paired-confirm-original.json",
        "paired-confirm-round3.json",
        "paired-confirm-round4.json",
        "paired-confirm-round5.json",
    )
]
baseline_hashes = {report["baseline_sha256"] for report in reports}
candidate_hashes = {report["candidate_sha256"] for report in reports}
fixture_hashes = {report["fixture_sha256"] for report in reports}
if len(baseline_hashes) != 1 or len(candidate_hashes) != 1 or len(fixture_hashes) != 1:
    raise RuntimeError("paired reports do not share binary and fixture hashes")
pairs = [pair for report in reports for pair in report["pairs"]]
if len(pairs) != 66 or not all(pair["valid"] and pair["output_equal"] for pair in pairs):
    raise RuntimeError("combined evidence must contain 66 valid, output-matching pairs")


def bootstrap(values, seed):
    rng = random.Random(seed)
    draws = sorted(statistics.median(rng.choices(values, k=len(values))) for _ in range(100_000))
    return [draws[2500], draws[97499]]


score_deltas = [pair["score_delta"] for pair in pairs]
rss_deltas = [pair["rss_delta_bytes"] for pair in pairs]
baseline_scores = [pair["baseline_score"] for pair in pairs]
candidate_scores = [pair["candidate_score"] for pair in pairs]
baseline_rss = [pair["baseline_rss_bytes"] for pair in pairs]
candidate_rss = [pair["candidate_rss_bytes"] for pair in pairs]
summary = {
    "experiment": "EarleyBoyer LoadConst -> LoadCapture, combined exact-baseline pairs",
    "baseline_sha256": next(iter(baseline_hashes)),
    "candidate_sha256": next(iter(candidate_hashes)),
    "fixture_sha256": next(iter(fixture_hashes)),
    "rounds": len(pairs),
    "bootstrap_replicates": 100_000,
    "baseline_score_median": statistics.median(baseline_scores),
    "candidate_score_median": statistics.median(candidate_scores),
    "score_delta_median": statistics.median(score_deltas),
    "score_delta_95_ci": bootstrap(score_deltas, 0x610ecb7),
    "score_wins": sum(delta > 0 for delta in score_deltas),
    "score_ties": sum(delta == 0 for delta in score_deltas),
    "score_losses": sum(delta < 0 for delta in score_deltas),
    "baseline_maximum_rss_median_bytes": statistics.median(baseline_rss),
    "candidate_maximum_rss_median_bytes": statistics.median(candidate_rss),
    "maximum_rss_delta_median_bytes": statistics.median(rss_deltas),
    "maximum_rss_delta_95_ci_bytes": bootstrap(rss_deltas, 0x610ecb8),
    "rss_lower_pairs": sum(delta < 0 for delta in rss_deltas),
    "rss_ties": sum(delta == 0 for delta in rss_deltas),
    "rss_higher_pairs": sum(delta > 0 for delta in rss_deltas),
    "valid": True,
    "output_equal": True,
    "qualification_ready": False,
    "pairs": pairs,
}
(OUT / "paired-combined-66.json").write_text(json.dumps(summary, indent=2) + "\n")
with (OUT / "pairs-combined-66.jsonl").open("w") as output:
    for pair in pairs:
        output.write(json.dumps(pair, separators=(",", ":")) + "\n")
print(json.dumps({key: value for key, value in summary.items() if key != "pairs"}, indent=2))
