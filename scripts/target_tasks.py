#!/usr/bin/env python3
"""Add targeting metadata to tasks/index.json for JS/TS 100% compat work.

Run from the repo root:
    python3 scripts/target_tasks.py

The script reads tasks/index.json, infers suite/category/batch/target_subset/
blocked_by/exit_criteria from each task's title and notes, writes the file
back, and prints a summary. It exits non-zero if any compatibility task is
missing required targeting fields.
"""
import json
import re
import sys
from pathlib import Path

INDEX = Path("tasks/index.json")

SUITE_KEYWORDS = {
    "test262": ["test262"],
    "typescript": ["typescript", "type script", "ts conformance", "ts scenario"],
    "harness": ["harness"],
    "runtime": ["runtime", "interpreter", "memory", "garbage", "gc", "performance", "optimize", "shape", "hir"],
    "tooling": ["linter", "lint", "linting", "script", "hot reload", "bridge", "host", "build", "green build", "cargo"],
}

CATEGORY_KEYWORDS = {
    "harness": ["harness"],
    "measurement": ["measure", "skip list", "report", "conformance runner", "negative-test"],
    "expressions": ["expression", "template literal", "spread", "nullish", "optional chaining", "computed key", "delete", "unary", "typeof"],
    "statements": ["statement", "for-of", "for-in", "do...while", "switch", "labeled", "with", "while", "strict mode", "use strict"],
    "functions": ["function", "arguments", "default parameter", "length property", "name property", "arrow"],
    "classes": ["class", "super", "extends", "static field", "private field"],
    "built-ins": ["constructor", "array", "error", "date", "boolean", "number", "string", "regexp", "bigint", "symbol", "parseint", "parsefloat", "json.stringify", "built-ins", "builtin", "built in"],
    "objects": ["object", "property descriptor", "defineproperty", "prototype", "insertion order", "own keys", "hasownproperty"],
    "errors": ["error", "throw", "try/catch", "exception", "tdz", "hoisting"],
    "modules": ["module", "import", "export", "moduleresolution"],
    "async": ["promise", "async", "await", "generator", "microtask"],
    "types": ["type", "ts type", "type literal", "type query", "nonprimitive"],
    "testing": ["unit test", "unit-test", "fixture", "scenario test", "testing strategy"],
    "jsx": ["jsx", "tsx"],
    "interpreter": ["interpreter", "trampoline", "stack overflow", "scope.rs"],
    "refactor": ["refactor", "split", "delete dead", "cleanup"],
}

# Language-level categories advance the spec suites; runtime/tooling categories do not.
LANGUAGE_CATEGORIES = {
    "expressions", "statements", "functions", "classes", "built-ins",
    "objects", "errors", "modules", "async", "types", "jsx",
}

# When two markdown files claim the same task id, prefer the canonical file.
CANONICAL_FILES = {
    "309": "309-expressions-coverage.md",
    "310": "310-statements-coverage.md",
}

# Explicit overrides for tasks whose title/notes are ambiguous.
MANUAL_OVERRIDES = {
    "296": {
        "suite": "both",
        "category": "measurement",
        "batch": 7,
        "target_subset": "Full tests/test262 (~53k files) + tests/typescript (~18k cases) conformance suites",
        "exit_criteria": "Both full conformance suites pass at 100% with zero spec skips.",
    },
    "250": {"category": "errors"},
    "320": {"category": "objects"},
    "286": {"suite": "both", "category": "objects", "priority": "P0"},
    "287": {"suite": "tooling", "category": "refactor", "priority": "P0"},
    "330": {"suite": "tooling", "category": "refactor"},
    "331": {"suite": "tooling", "category": "testing"},
    "332": {"suite": "tooling", "category": "testing"},
    "334": {"suite": "harness", "category": "measurement", "batch": 0},
    "335": {"suite": "both", "category": "objects", "batch": 1, "priority": "P0"},
    "336": {"suite": "both", "category": "built-ins", "batch": 2},
    "337": {"suite": "tooling", "category": "testing", "batch": 6},
    "338": {"suite": "runtime", "category": "interpreter", "priority": "P0"},
    "342": {"suite": "tooling", "category": "testing"},
    "344": {"priority": "P0", "suite": "both", "category": "harness", "batch": 0},
    "87": {"suite": "tooling", "category": "refactor"},
    "85": {"category": "interpreter", "priority": "P0"},
    "88": {"priority": "P0"},
    "264": {"priority": "P0"},
    "285": {"priority": "P0"},
    "308": {"priority": "P0"},
    "333": {"priority": "P0"},
    "343": {"priority": "P0"},
}

# Explicit batch overrides for well-known cross-cutting tasks.
BATCH_OVERRIDES = {
    "253": 0,
    "91": 0,
    "250": 0,
    "97": 0,
    "344": 0,
    # VM/runtime foundation is Batch 1: build the proper runtime before the
    # 100% language-compat crunch.
    "85": 1,
    "88": 1,
    "264": 1,
    "285": 1,
    "286": 1,
    "287": 1,
    "308": 1,
    "333": 1,
    "335": 1,
    "338": 1,
    "343": 1,
    # Big language features shift to Batch 4 (after VM foundation + syntax +
    # functions/core-statements batches).
    "241": 4,
    "182": 4,
    "183": 4,
    "187": 4,
    "251": 4,
    # Full-suite / host-polish milestones come after language-compat crunch.
    "82": 7,
    "256": 7,
    "296": 7,
}


def _matches(text: str, keyword: str) -> bool:
    return re.search(r"\b" + re.escape(keyword.lower()) + r"\b", text.lower()) is not None


def infer_suite(text: str, category: str) -> str:
    for suite, kws in SUITE_KEYWORDS.items():
        if any(_matches(text, kw) for kw in kws):
            return suite
    if category in LANGUAGE_CATEGORIES:
        return "both"
    if category in {"harness", "measurement"}:
        return "harness"
    if category == "tooling":
        return "tooling"
    return "runtime"


def infer_category(text: str) -> str:
    for category, kws in CATEGORY_KEYWORDS.items():
        if any(_matches(text, kw) for kw in kws):
            return category
    return "refactor"


def infer_batch(task: dict) -> int:
    tid = task["id"]
    if tid in BATCH_OVERRIDES:
        return BATCH_OVERRIDES[tid]
    priority = task.get("priority", "P2")
    category = task.get("category", "refactor")
    if priority == "P0":
        if category in {"harness", "measurement"}:
            return 0
        # VM/runtime hardening is Batch 1 (the foundation). All language-compat
        # P0 work is shifted one batch later.
        if category in {"expressions", "built-ins", "objects", "errors"}:
            return 2
        if category in {"statements", "functions", "modules"}:
            return 3
        return 4
    if priority == "P1":
        return 5
    if priority == "P2":
        return 6
    return 7


def target_subset_for(category: str, suite: str) -> str:
    if suite == "harness":
        return "tests/test262/harness/ + tests/typescript/tests/cases/conformance harness integration"
    if category == "measurement":
        return "target/test262_report.md and target/conformance_report.md accuracy"
    if suite == "both":
        return f"tests/test262 + tests/typescript {category}/ subset"
    if suite in {"test262", "typescript"}:
        return f"tests/{suite} {category}/ subset"
    return "n/a (runtime/tooling task)"


def exit_criteria_for(task: dict) -> str:
    suite = task.get("suite", "runtime")
    category = task.get("category", "refactor")
    if suite in {"test262", "typescript", "both"}:
        return f"Relevant {suite} {category} subset passes at 100% with zero spec skips and the harness report is regenerated."
    if suite == "harness":
        return "Harness loads real helpers, reports accurate pass/fail/skip counts, and no test is skipped due to missing helper stubs."
    if suite == "runtime":
        return "No conformance regression; relevant runtime invariants are covered by unit tests."
    return "Task acceptance criteria in the task file are met."


def blocked_by_for(tid: str) -> list:
    """Return known explicit dependencies. Avoid adding speculative deps."""
    deps = []
    if tid.startswith("290") and tid != "290":
        deps.append("290")
    if tid in {"322a", "322b"}:
        deps.append("322")
    if tid in {"289a", "289b", "289c"}:
        deps.append("289")
    return deps


def _natural_id_sort_key(task_id: str):
    match = re.match(r"^(\d+)([a-z]*)$", task_id)
    if match:
        return (int(match.group(1)), match.group(2))
    return (float("inf"), task_id)


def _parse_task_file(path: Path) -> dict:
    text = path.read_text()
    lines = text.splitlines()
    task_id = path.stem.split("-")[0]

    title = path.stem.replace("-", " ").title()
    for line in lines:
        line = line.strip()
        m = re.match(r"^#\s+Task\s+\S+:\s+(.+)$", line)
        if m:
            title = m.group(1).strip()
            break
        m = re.match(r"^#\s+(.+)$", line)
        if m:
            title = m.group(1).strip()
            break

    status = "pending"
    for line in lines:
        m = re.match(r"^##\s*Status:\s*(.+)$", line, re.IGNORECASE)
        if m:
            raw = m.group(1).strip().lower()
            if raw in {"completed", "done"}:
                status = "completed"
            elif raw in {"closed", "cancelled", "canceled"}:
                status = "closed"
            elif raw in {"in progress", "in-progress", "in_progress"}:
                status = "in_progress"
            else:
                status = "pending"
            break

    notes = ""
    in_notes = False
    for line in lines:
        if re.match(r"^##\s*Notes", line, re.IGNORECASE):
            in_notes = True
            continue
        if in_notes:
            if re.match(r"^##\s+", line):
                break
            if line.strip():
                notes = line.strip()
                break
    if not notes:
        for line in lines:
            if re.match(r"^##\s*Goal", line, re.IGNORECASE):
                continue
            stripped = line.strip()
            if stripped and not stripped.startswith("#"):
                notes = stripped
                break

    return {
        "id": task_id,
        "title": title,
        "file": str(path),
        "status": status,
        "priority": "P2",
        "notes": notes,
    }


def main() -> int:
    if not INDEX.exists():
        print(f"error: {INDEX} not found", file=sys.stderr)
        return 1

    data = json.loads(INDEX.read_text())
    tasks = data["tasks"]

    # Prune entries whose markdown file no longer exists (e.g., renamed/removed tasks).
    pruned = [t for t in tasks if Path(t["file"]).exists()]
    if len(pruned) != len(tasks):
        print(f"Pruned {len(tasks) - len(pruned)} missing task file(s).")
        tasks = pruned

    by_id = {t["id"]: t for t in tasks}

    # Re-sync status and title from the task files so the registry stays honest.
    for task in tasks:
        try:
            parsed = _parse_task_file(Path(task["file"]))
            task["status"] = parsed["status"]
            task["title"] = parsed["title"]
        except FileNotFoundError:
            print(f"warning: registered file not found: {task['file']}", file=sys.stderr)

    # Register any task markdown files not yet in the index.
    registered = set(by_id.keys())
    duplicate_files = {}

    # Group markdown files by task id so we can detect duplicates.
    files_by_id = {}
    for path in Path("tasks").glob("*.md"):
        task_id = path.stem.split("-")[0]
        files_by_id.setdefault(task_id, []).append(path)

    for task_id, paths in sorted(files_by_id.items()):
        if task_id in registered:
            # Already in the registry; report any extra files claiming the same id.
            existing_file = Path(by_id[task_id]["file"]).name
            extras = [p for p in paths if p.name != existing_file]
            if extras:
                duplicate_files[task_id] = [str(p) for p in extras]
            continue

        # Choose the canonical file if a duplicate id exists.
        if task_id in CANONICAL_FILES:
            canonical_name = CANONICAL_FILES[task_id]
            chosen = next((p for p in paths if p.name == canonical_name), paths[0])
            skipped = [p for p in paths if p != chosen]
        elif len(paths) > 1:
            chosen = paths[0]
            skipped = paths[1:]
        else:
            chosen = paths[0]
            skipped = []

        if skipped:
            duplicate_files[task_id] = [str(p) for p in skipped]

        task = _parse_task_file(chosen)
        tasks.append(task)
        by_id[task_id] = task
        registered.add(task_id)

    unclassified = []

    for task in tasks:
        text = f"{task.get('title', '')} {task.get('notes', '')}"
        task["category"] = infer_category(text)
        task["suite"] = infer_suite(text, task["category"])

        # Apply manual overrides, then compute derived fields; preserve any
        # override values for batch, target_subset, or exit_criteria.
        override = MANUAL_OVERRIDES.get(task["id"], {})
        task.update(override)

        task["batch"] = override.get("batch", infer_batch(task))
        task["target_subset"] = override.get("target_subset", target_subset_for(task["category"], task["suite"]))
        task["blocked_by"] = blocked_by_for(task["id"])
        task["exit_criteria"] = override.get("exit_criteria", exit_criteria_for(task))
        if task["category"] == "refactor" and task["suite"] == "runtime":
            unclassified.append(task["id"])

    # Sort the registry by batch, then by numeric task id.
    data["tasks"] = sorted(
        tasks,
        key=lambda t: (t["batch"], _natural_id_sort_key(t["id"])),
    )

    INDEX.write_text(json.dumps(data, indent=2) + "\n")

    print(f"Updated {len(tasks)} tasks.")
    if duplicate_files:
        print("\nSkipped duplicate-id files (review and rename/remove):")
        for task_id, files in sorted(duplicate_files.items()):
            for f in files:
                print(f"  {task_id}: {f}")
    if unclassified:
        print(f"\nPotentially unclassified tasks (review manually): {unclassified}")

    by_batch = {}
    by_suite = {}
    for task in tasks:
        by_batch.setdefault(task["batch"], 0)
        by_batch[task["batch"]] += 1
        by_suite.setdefault(task["suite"], 0)
        by_suite[task["suite"]] += 1

    print("\nBy batch:")
    for batch in sorted(by_batch):
        print(f"  Batch {batch}: {by_batch[batch]} tasks")

    print("\nBy suite:")
    for suite in sorted(by_suite):
        print(f"  {suite}: {by_suite[suite]} tasks")

    # Validation: every compat task must have a concrete target_subset and exit_criteria.
    violations = []
    for task in tasks:
        if task["suite"] in {"test262", "typescript", "both"}:
            if not task.get("target_subset") or task["target_subset"].startswith("n/"):
                violations.append((task["id"], "missing target_subset"))
            if not task.get("exit_criteria"):
                violations.append((task["id"], "missing exit_criteria"))

    if violations:
        print("\nTargeting violations:")
        for tid, reason in violations:
            print(f"  {tid}: {reason}")
        return 1

    print("\nAll compatibility tasks are targeted.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
