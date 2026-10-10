#!/usr/bin/env python3
"""Derive per-function self-Ir deltas from the four Richards profiles."""

from __future__ import annotations

import gzip
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).parent
BASELINE_SHA = "ec65a4e93b7db01de2acbf0b67a1ec357a21ae33472153a23615961834b5e326"
CANDIDATE_SHA = "e3321d6d06cac1552f15f9e45d2d36e87ed693b9ef8cff4b408da22af1d115fc"
SOURCE_DIFF_SHA = "a9597d2046b32a98a99eda0f0fa1a1221ccdbb3c8fc76d89c31454504826200a"
SOURCE_BASE = "a03e0646ff216a192d85c6d1d22307855cdb21f2"


def canonical_name(name: str) -> str:
    stem, separator, suffix = name.rpartition("'")
    return stem if separator and suffix.isdigit() else name


def position_cost(fields: list[str]) -> int | None:
    if len(fields) != 2 or not fields[1].isdigit():
        return None
    position = fields[0]
    valid = position == "*" or position.isdigit()
    valid |= position[:1] in ("+", "-") and position[1:].isdigit()
    return int(fields[1]) if valid else None


def parse_profile(path: Path) -> tuple[int, dict[str, int], dict[tuple[str, str], int]]:
    opener = gzip.open if path.suffix == ".gz" else open
    names: dict[str, str] = {}
    self_by_id: dict[str, int] = {}
    calls_by_id: dict[tuple[str, str], int] = {}
    current = pending_callee = None
    edge_cost = False
    summary = None
    with opener(path, "rt", errors="replace") as source:
        for line in source:
            line = line.rstrip("\n")
            if line.startswith("summary:"):
                summary = int(line.split()[1])
            elif line.startswith(("fn=(", "cfn=(")):
                kind, rest = line.split("=(", 1)
                ident, _, name = rest.partition(")")
                if name.strip():
                    names[ident] = name.strip()
                if kind == "fn":
                    current = ident
                else:
                    pending_callee = ident
            elif line.startswith("calls="):
                count = int(line[6:].split()[0])
                edge = (current, pending_callee)
                calls_by_id[edge] = calls_by_id.get(edge, 0) + count
                edge_cost = True
            else:
                cost = position_cost(line.split())
                if cost is not None:
                    if edge_cost:
                        edge_cost = False
                    elif current is not None:
                        self_by_id[current] = self_by_id.get(current, 0) + cost
    if summary is None or sum(self_by_id.values()) != summary:
        raise ValueError(f"{path.name}: per-function self-Ir does not match summary")
    self_ir: dict[str, int] = {}
    for ident, cost in self_by_id.items():
        name = canonical_name(names.get(ident, f"<unnamed:{ident}>"))
        self_ir[name] = self_ir.get(name, 0) + cost
    calls: dict[tuple[str, str], int] = {}
    for (caller, callee), count in calls_by_id.items():
        edge = tuple(canonical_name(names.get(ident or "", f"<unnamed:{ident}>"))
                     for ident in (caller, callee))
        calls[edge] = calls.get(edge, 0) + count
    return summary, self_ir, calls


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parsed = {
        label: parse_profile(ROOT / f"callgrind-richards-{label}.out.gz")
        for label in ("baseline-k0", "baseline-k1", "candidate-k0", "candidate-k1")
    }
    work: dict[str, dict[str, int]] = {}
    for build in ("baseline", "candidate"):
        before = parsed[f"{build}-k0"][1]
        after = parsed[f"{build}-k1"][1]
        work[build] = {name: after.get(name, 0) - before.get(name, 0)
                       for name in before.keys() | after.keys()}
    all_names = work["baseline"].keys() | work["candidate"].keys()
    delta = {name: work["candidate"].get(name, 0) - work["baseline"].get(name, 0)
             for name in all_names}
    work_ir = {build: parsed[f"{build}-k1"][0] - parsed[f"{build}-k0"][0]
               for build in ("baseline", "candidate")}
    inputs = {f"k{count}": sha256(ROOT / f"callgrind-richards-k{count}.js")
              for count in (0, 1)}
    result = {
        "schema": 1,
        "method": "Callgrind Ir self costs, K=1 minus K=0 per binary; names are resolved from fn/cfn definitions and context suffixes are combined.",
        "source_base": SOURCE_BASE,
        "candidate_source_diff_sha256": SOURCE_DIFF_SHA,
        "baseline_binary_sha256": BASELINE_SHA,
        "candidate_binary_sha256": CANDIDATE_SHA,
        "input_sha256": inputs,
        "profile_sha256": {
            label: sha256(ROOT / f"callgrind-richards-{label}.out.gz")
            for label in parsed
        },
        "profile_totals": {label: value[0] for label, value in parsed.items()},
        "work_ir": work_ir,
        "work_delta_ir": work_ir["candidate"] - work_ir["baseline"],
        "work_delta_percent": (work_ir["candidate"] / work_ir["baseline"] - 1) * 100,
        "targeted_call_counts": {
            name: {
                build: sum(count for (caller, callee), count in parsed[f"{build}-k1"][2].items()
                           if callee == name) - sum(
                    count for (caller, callee), count in parsed[f"{build}-k0"][2].items()
                    if callee == name)
                for build in ("baseline", "candidate")
            }
            for name in ("<quench_runtime::heap::Heap>::object",
                         "<quench_runtime::heap::Heap>::object_mut")
        },
        "top_candidate_self_ir": sorted(work["candidate"].items(),
                                          key=lambda item: item[1], reverse=True)[:30],
        "top_baseline_self_ir": sorted(work["baseline"].items(),
                                         key=lambda item: item[1], reverse=True)[:30],
        "largest_positive_candidate_minus_baseline": sorted(
            delta.items(), key=lambda item: item[1], reverse=True)[:40],
        "largest_negative_candidate_minus_baseline": sorted(
            delta.items(), key=lambda item: item[1])[:20],
    }
    output = ROOT / "callgrind-richards-attribution.json"
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({key: value for key, value in result.items()
                      if key not in ("top_candidate_self_ir", "top_baseline_self_ir",
                                     "largest_positive_candidate_minus_baseline",
                                     "largest_negative_candidate_minus_baseline")}, indent=2))
    for name, value in result["largest_positive_candidate_minus_baseline"][:20]:
        print(f"{value:>12,} {name}")


if __name__ == "__main__":
    main()
