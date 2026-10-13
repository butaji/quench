#!/usr/bin/env python3
"""Subtract setup-only aggregate counters and compare candidate with trunk."""

import json
from pathlib import Path


HERE = Path(__file__).resolve().parent
WORK = {"richards-k100.js": 100, "regexp-k1.js": 1}
COUNTER_GROUPS = (
    "field_cache",
    "indexed_access",
    "calls",
    "terminal_calls",
    "array_writes",
    "shape_transitions",
    "string_concats",
    "allocation_kinds",
    "gc",
)


def numeric_delta(work, setup):
    if isinstance(work, bool):
        return None
    if isinstance(work, (int, float)):
        return work - setup if isinstance(setup, (int, float)) else None
    if isinstance(work, list):
        return [numeric_delta(w, s) for w, s in zip(work, setup)]
    if isinstance(work, dict):
        return {
            key: numeric_delta(value, setup.get(key))
            for key, value in work.items()
            if key in setup
        }
    return None


def counters(label: str, fixture: str) -> dict:
    data = json.loads((HERE / f"{label}-aggregate-counters.json").read_text())
    return next(
        item
        for item in data["reports"][fixture]["reports"]
        if item["kind"] == "quench-profile"
    )


def delta(label: str, work_file: str, setup_file: str) -> dict:
    work = counters(label, work_file)
    setup = counters(label, setup_file)
    iterations = WORK[work_file]
    result = {
        "source_label": label,
        "input_work_sha256": json.loads(
            (HERE / f"{label}-aggregate-counters.json").read_text()
        )["reports"][work_file]["input_sha256"],
        "iterations": iterations,
        "counters_per_run": {},
    }
    for group in COUNTER_GROUPS:
        result["counters_per_run"][group] = scale(
            numeric_delta(work[group], setup[group]), iterations
        )
    result["counters_per_run"]["allocations"] = (
        work["allocations"] - setup["allocations"]
    ) / iterations
    result["counters_per_run"]["collections"] = (
        work["collections"] - setup["collections"]
    ) / iterations
    result["peak_live_after_minus_setup"] = work["peak_live"] - setup["peak_live"]
    opcodes = scale(numeric_delta(work["opcodes"], setup["opcodes"]), iterations)
    result["counters_per_run"]["opcodes"] = {
        name: count for name, count in opcodes.items() if count
    }
    return result


def scale(value, divisor):
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return value / divisor if divisor else value
    if isinstance(value, list):
        return [scale(item, divisor) for item in value]
    if isinstance(value, dict):
        return {key: scale(item, divisor) for key, item in value.items()}
    return None


def main() -> None:
    fixtures = {
        "richards.js": ("richards-k100.js", "richards-k0.js"),
        "regexp.js": ("regexp-k1.js", "regexp-k0.js"),
    }
    output = {fixture: {} for fixture in fixtures}
    for fixture, (work_file, setup_file) in fixtures.items():
        output[fixture]["trunk"] = delta("trunk", work_file, setup_file)
        output[fixture]["candidate"] = delta("candidate", work_file, setup_file)
    path = HERE / "setup-subtracted-counters.json"
    path.write_text(json.dumps(output, indent=2) + "\n")
    print(path)


if __name__ == "__main__":
    main()
