#!/usr/bin/env python3
"""Collect one exact-input physical-opcode census per Linux V8-v7 fixture."""
from __future__ import annotations

import hashlib
import argparse
import json
import os
import platform
import re
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SUITE = ROOT / "quench-bench/js-engine-benchmark/v8-v7"
FIXTURES = [
    "crypto.js", "deltablue.js", "earley-boyer.js", "navier-stokes.js",
    "raytrace.js", "regexp.js", "richards.js", "splay.js",
]
BINARY = ROOT / "target/iteration/quench-node"
OUT = ROOT / "tasks/evidence/task61-dispatch-census-linux-2026-10-09.json"
INPUT_DIR = ROOT / "target/iteration/dispatch-census-inputs"
PROFILE_FILES = [
    "crates/quench-runtime/src/profile.rs",
    "crates/quench-runtime/src/vm/dispatch_numeric.rs",
    "crates/quench-runtime/src/vm.rs",
]


def run(args: list[str], **kwargs):
    return subprocess.run(args, check=True, text=True, stdout=subprocess.PIPE, **kwargs)


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_sha(path: Path) -> str:
    return sha(path.read_bytes())


def git(*args: str) -> str:
    return run(["git", *args], cwd=ROOT).stdout.strip()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixtures", nargs="+", choices=FIXTURES, default=FIXTURES)
    parser.add_argument("--out", type=Path, default=OUT)
    args = parser.parse_args()
    INPUT_DIR.mkdir(parents=True, exist_ok=True)
    rust_source = (ROOT / "quench-bench/src/main.rs").read_text()
    match = re.search(r'const RUNNER: &str = r#"(.*?)"#;', rust_source, re.S)
    if not match:
        raise RuntimeError("could not extract the quench-bench runner")
    runner = match.group(1).encode()
    base = (SUITE / "base.js").read_bytes()
    source_patch = run(
        ["git", "diff", "--", *PROFILE_FILES], cwd=ROOT
    ).stdout.encode()
    env = {key: os.environ[key] for key in ("PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "TZ") if key in os.environ}
    env["QUENCH_OPCODE_CENSUS"] = "1"
    records = {}
    for fixture in args.fixtures:
        fixture_bytes = (SUITE / fixture).read_bytes()
        source = base + b"\n" + fixture_bytes + b"\n" + runner
        input_path = INPUT_DIR / fixture
        input_path.write_bytes(source)
        started = time.monotonic()
        result = subprocess.run(
            [str(BINARY), str(input_path)], env=env, cwd=ROOT,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300,
        )
        elapsed = time.monotonic() - started
        lines = [line for line in result.stderr.splitlines() if line.startswith('{"kind":"quench-dispatch-opcode-census"')]
        if result.returncode != 0 or len(lines) != 1:
            raise RuntimeError(
                f"{fixture}: status={result.returncode}, census_records={len(lines)}, stderr={result.stderr[-2000:]}"
            )
        census = json.loads(lines[0])
        if census["total"] != census["dispatch_sites"]:
            raise RuntimeError(f"{fixture}: dispatch counters differ")
        scores = re.findall(r"^Score: (.+)$", result.stdout, re.M)
        records[fixture.removesuffix(".js")] = {
            "input_sha256": sha(source),
            "input_size_bytes": len(source),
            "status": result.returncode,
            "elapsed_seconds": round(elapsed, 3),
            "stdout_sha256": sha(result.stdout.encode()),
            "score_lines": scores,
            **census,
        }
        print(f"{fixture}: dispatches={census['total']:,}; score={scores}; {elapsed:.1f}s", flush=True)

    cpu = "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text(errors="replace").splitlines():
            if line.lower().startswith("model name"):
                cpu = line.partition(":")[2].strip()
                break
    report = {
        "schema": 1,
        "kind": "task61-v8-v7-physical-dispatch-opcode-census",
        "date": "2026-10-09",
        "source_revision": git("rev-parse", "HEAD"),
        "instrumentation": {
            "files": PROFILE_FILES,
            "worktree_patch_sha256": sha(source_patch),
            "file_sha256": {name: file_sha(ROOT / name) for name in PROFILE_FILES},
        },
        "profile_binary": {
            "path": "target/iteration/quench-node",
            "sha256": file_sha(BINARY),
            "features": ["profile-aggregate"],
            "build_command": "cargo build --profile iteration -p quench-node --bin quench-node --features quench-runtime/profile-aggregate",
            "counter_mode": "QUENCH_OPCODE_CENSUS=1; physical dispatch counts exclude virtual fused-op accounting",
            "run_command_template": "QUENCH_OPCODE_CENSUS=1 target/iteration/quench-node <exact materialized fixture>",
        },
        "host": {
            "os": platform.platform(),
            "architecture": platform.machine(),
            "cpu": cpu,
            "logical_cpus": os.cpu_count(),
        },
        "method": "One instrumented run per fixture. Materialization exactly matches quench-bench: base.js + LF + fixture + LF + the Rust runner constant. Physical dispatch events and separately recorded physical dispatch-site events must agree. Numeric local-update fusion records logical opcode events through fused_opcode, so skipped IncDec/StoreLocal steps are excluded from the physical census. Census counts are diagnostic and are not performance measurements.",
        "fixtures": records,
        "online_research": {
            "source": "https://v8.dev/blog/ignition-interpreter",
            "finding": "V8 describes bytecode peephole passes that replace common patterns, remove redundant operations, and reduce register transfers. This motivates looking for frequent local/opcode sequences; it does not validate a Quench speedup.",
            "accessed_utc": datetime.now(timezone.utc).isoformat(),
        },
    }
    report["selected_fixtures"] = args.fixtures
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"wrote {args.out}", flush=True)


if __name__ == "__main__":
    main()
