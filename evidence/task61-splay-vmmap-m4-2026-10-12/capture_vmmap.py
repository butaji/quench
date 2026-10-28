#!/usr/bin/env python3
"""Capture uninstrumented Splay VM maps after setup and after fixed work."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import selectors
import signal
import subprocess
import time
from pathlib import Path

EXPECTED_BINARY_SHA256 = "afde285b7f8dff56da12f44eb915034b0599d7aec95a2ff39d55ae18a8d7c4cf"
EXPECTED_BASE_SHA256 = "ac383759c3cbf5f3a499ade526a4e296f7925733d8c3889b6d5840ce6867ff51"
EXPECTED_FIXTURE_SHA256 = "1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d"
WORK_RUNS = 300
MARKER_TIMEOUT_SECONDS = 90
VM_MAP_TIMEOUT_SECONDS = 30


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runner(iterations: int, marker: str) -> bytes:
    source = f"""
var __quenchMemorySuites = BenchmarkSuite.suites;
var __quenchMemoryPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
if (__quenchMemorySuites.length !== 1) throw new Error("fixed-work suite count mismatch");
var __quenchMemorySuite = __quenchMemorySuites[0];
if (__quenchMemorySuite.name !== "Splay") throw new Error("fixed-work suite mismatch");
if (__quenchMemorySuite.benchmarks.length !== 1) throw new Error("fixed-work benchmark count mismatch");
var __quenchMemoryBenchmark = __quenchMemorySuite.benchmarks[0];
__quenchMemoryBenchmark.Setup();
for (var __quenchMemoryRun = 0; __quenchMemoryRun < {iterations}; __quenchMemoryRun++) {{
  __quenchMemoryBenchmark.run();
}}
__quenchMemoryPrint("{marker}");
while (true) {{}}
"""
    return source.encode()


def materialize(base: Path, fixture: Path, output: Path, iterations: int, marker: str) -> None:
    output.write_bytes(base.read_bytes() + b"\n" + fixture.read_bytes() + b"\n" + runner(iterations, marker))


def capture_stage(binary: Path, source: Path, output_dir: Path, stage: str, marker: str) -> dict[str, object]:
    stdout_path = output_dir / f"{stage}.stdout.txt"
    stderr_path = output_dir / f"{stage}.stderr.txt"
    vmmap_path = output_dir / f"{stage}.vmmap.txt"
    summary_path = output_dir / f"{stage}.vmmap-summary.txt"
    process_info_path = output_dir / f"{stage}.process.txt"
    env = {
        "HOME": os.environ.get("HOME", "/Users/admin"),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "PATH": os.environ.get("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
    }
    started = time.time()
    with stderr_path.open("wb") as stderr_file:
        child = subprocess.Popen([str(binary), str(source)], stdout=subprocess.PIPE, stderr=stderr_file, env=env)
        assert child.stdout is not None
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ)
        captured = bytearray()
        deadline = time.monotonic() + MARKER_TIMEOUT_SECONDS
        marker_bytes = marker.encode()
        try:
            while marker_bytes not in captured and time.monotonic() < deadline:
                for key, _ in selector.select(timeout=0.25):
                    chunk = os.read(key.fileobj.fileno(), 4096)
                    if not chunk:
                        raise RuntimeError(f"process exited before marker {marker!r}: {child.poll()}")
                    captured.extend(chunk)
            if marker_bytes not in captured:
                raise TimeoutError(f"timed out waiting for marker {marker!r}")
            os.kill(child.pid, signal.SIGSTOP)
            time.sleep(0.05)
            vmmap = subprocess.run(
                ["vmmap", str(child.pid)], capture_output=True, text=True, timeout=VM_MAP_TIMEOUT_SECONDS, check=False
            )
            summary = subprocess.run(
                ["vmmap", "-summary", str(child.pid)],
                capture_output=True,
                text=True,
                timeout=VM_MAP_TIMEOUT_SECONDS,
                check=False,
            )
            process_info = subprocess.run(
                ["ps", "-o", "pid=,rss=,vsz=,state=,etime=,command=", "-p", str(child.pid)],
                capture_output=True,
                text=True,
                timeout=VM_MAP_TIMEOUT_SECONDS,
                check=False,
            )
            stdout_path.write_bytes(captured)
            vmmap_path.write_text(vmmap.stdout + vmmap.stderr)
            summary_path.write_text(summary.stdout + summary.stderr)
            process_info_path.write_text(process_info.stdout + process_info.stderr)
            if vmmap.returncode != 0 or summary.returncode != 0 or process_info.returncode != 0:
                raise RuntimeError(f"snapshot command failed for {stage}; inspect the raw outputs")
            return {
                "stage": stage,
                "pid": child.pid,
                "source": str(source),
                "source_sha256": sha256(source),
                "captured_unix_seconds": started,
                "marker": marker,
                "vmmap_exit": vmmap.returncode,
                "summary_exit": summary.returncode,
                "process_info_exit": process_info.returncode,
                "files": [str(path) for path in (stdout_path, stderr_path, vmmap_path, summary_path, process_info_path)],
            }
        finally:
            selector.close()
            if child.poll() is None:
                child.kill()
            child.wait()
            child.stdout.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()

    binary = args.binary.resolve()
    corpus = args.corpus_root.resolve()
    base = corpus / "base.js"
    fixture = corpus / "splay.js"
    if sha256(binary) != EXPECTED_BINARY_SHA256:
        raise SystemExit("candidate binary SHA-256 does not match the pinned Splay record candidate")
    if sha256(base) != EXPECTED_BASE_SHA256 or sha256(fixture) != EXPECTED_FIXTURE_SHA256:
        raise SystemExit("V8-v7 base.js or splay.js hash differs from the pinned input")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    setup_source = args.output_dir / "splay-setup-hold.js"
    work_source = args.output_dir / "splay-work-hold.js"
    materialize(base, fixture, setup_source, 0, "__quenchMemoryHold:setup")
    materialize(base, fixture, work_source, WORK_RUNS, "__quenchMemoryHold:work")
    stages = [
        capture_stage(binary, setup_source, args.output_dir, "setup", "__quenchMemoryHold:setup"),
        capture_stage(binary, work_source, args.output_dir, "work", "__quenchMemoryHold:work"),
    ]
    metadata = {
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "logical_cpus": os.cpu_count(),
            "load_average_before": os.getloadavg(),
        },
        "binary": str(binary),
        "binary_sha256": sha256(binary),
        "base_sha256": sha256(base),
        "fixture_sha256": sha256(fixture),
        "work_runs": WORK_RUNS,
        "capture_stages": stages,
        "method": "Stop the uninstrumented pinned process immediately after setup or 300 run() calls; capture vmmap and ps before teardown.",
        "limits": [
            "This is an OS mapping snapshot, not the profile-memory structural counter build.",
            "The work snapshot is after the final run(), before TearDown(); it may miss a transient intra-run RSS peak.",
        ],
    }
    (args.output_dir / "metadata.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
