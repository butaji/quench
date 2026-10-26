#!/usr/bin/env python3
"""Audit exact six-way outcomes in a run-parallel JSONL checkpoint."""
import collections
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
raw = path.read_bytes()
lines = raw.splitlines()
counts = collections.Counter()
fixtures = set()
inventory_hashes = set()
duplicates = []
skip_without_tap = []
for number, line in enumerate(lines, 1):
    try:
        row = json.loads(line)
    except json.JSONDecodeError:
        # A concurrent append can expose only its final incomplete line.
        if number == len(lines) and not raw.endswith(b"\n"):
            continue
        raise
    fixture = row.get("fixture", "<missing fixture>")
    if row.get("inventory_hash") is not None:
        inventory_hashes.add(row["inventory_hash"])
    if fixture in fixtures:
        duplicates.append(fixture)
    fixtures.add(fixture)
    result = row.get("observation")
    if not isinstance(result, dict) or "Ok" not in result:
        counts["unclassified"] += 1
        continue
    obs = result["Ok"]
    worker = obs.get("worker") or {}
    kind = worker.get("outcome")
    exit_code = obs.get("exit_code")
    if obs.get("timed_out"):
        outcome = "timeout"
    elif exit_code is None:
        outcome = "crash"
    elif kind == "pass" and exit_code == 0:
        outcome = "pass"
    elif kind == "guestexit" and exit_code == 0:
        output = bytes(obs.get("stdout", []))
        if worker.get("code") == 0 and output.startswith(b"1..0 # Skipped:"):
            outcome = "skip"
        elif worker.get("code") == 0:
            outcome = "pass"
        else:
            outcome = "fail"
    elif kind == "skip" and exit_code == 0:
        outcome = "skip"
    elif kind == "fail" and exit_code != 0:
        outcome = "fail"
    elif kind == "guestexit" and exit_code != 0:
        outcome = "fail"
    else:
        outcome = "unclassified"
    counts[outcome] += 1
    if outcome == "skip":
        output = bytes(obs.get("stdout", []))
        if not output.startswith(b"1..0 # Skipped:"):
            skip_without_tap.append(fixture)

print(json.dumps({
    "checkpoint": str(path),
    "rows": sum(counts.values()),
    "inventory_hashes": sorted(inventory_hashes),
    "counts": dict(sorted(counts.items())),
    "duplicate_fixtures": duplicates,
    "skips_without_tap_marker": skip_without_tap,
}, indent=2))
