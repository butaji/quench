#!/usr/bin/env python3
"""Run the materialized guard fixtures and retain aggregate counters only."""

import hashlib
import json
import os
import sys
from pathlib import Path
from subprocess import run


HERE = Path(__file__).resolve().parent
INPUTS = (
    "richards-k0.js",
    "richards-k100.js",
    "regexp-k0.js",
    "regexp-k1.js",
)
PROFILE_KINDS = {"quench-profile", "quench-gc-breakdown"}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    binary = Path(sys.argv[1]).resolve()
    label = sys.argv[2]
    environment = os.environ.copy()
    for name in ("QUENCH_OPCODE_CENSUS", "QUENCH_GC_TIME_PROFILE", "QUENCH_MEMORY"):
        environment.pop(name, None)

    reports = {}
    for name in INPUTS:
        input_path = HERE / name
        result = run(
            [str(binary), str(input_path)],
            env=environment,
            check=False,
            capture_output=True,
            text=True,
        )
        (HERE / f"{label}-{name}.stdout").write_text(result.stdout)
        (HERE / f"{label}-{name}.stderr").write_text(result.stderr)
        if result.returncode != 0:
            raise SystemExit(f"{label}/{name} exited {result.returncode}")
        if f"__quenchFixedWork:" not in result.stdout:
            raise SystemExit(f"{label}/{name} missed the fixed-work completion marker")
        parsed = []
        for line in result.stderr.splitlines():
            if line.startswith("{"):
                try:
                    item = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if item.get("kind") in PROFILE_KINDS:
                    parsed.append(item)
        if not any(item.get("kind") == "quench-profile" for item in parsed):
            raise SystemExit(f"{label}/{name} emitted no aggregate profile")
        reports[name] = {
            "input_sha256": sha256(input_path),
            "stdout_sha256": sha256(HERE / f"{label}-{name}.stdout"),
            "stderr_sha256": sha256(HERE / f"{label}-{name}.stderr"),
            "reports": parsed,
        }

    output = {
        "schema": 1,
        "source_label": label,
        "binary": str(binary),
        "binary_sha256": sha256(binary),
        "reports": reports,
    }
    report_path = HERE / f"{label}-aggregate-counters.json"
    report_path.write_text(json.dumps(output, indent=2) + "\n")
    print(report_path)


if __name__ == "__main__":
    main()
