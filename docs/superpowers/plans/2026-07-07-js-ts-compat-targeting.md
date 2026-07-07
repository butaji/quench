# JS/TS Compatibility Targeting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add targeting metadata to every task and update the compatibility docs so the entire project is organized into measurable batches aimed at 100% test262 + TypeScript conformance.

**Architecture:** Extend `tasks/index.json` with a small schema (`suite`, `category`, `batch`, `target_subset`, `blocked_by`, `exit_criteria`), infer the values for existing tasks with a script, then update `docs/js-ts-compatibility-roadmap.md` and `docs/conformance.md` to reference the batch taxonomy. Provide a validation/report script and apply it.

**Tech Stack:** Python 3 (for JSON transformation), Markdown, existing `tasks/index.json`.

---

### Task 1: Create the targeting transformation script

**Files:**
- Create: `scripts/target_tasks.py`

- [ ] **Step 1: Write the script**

Create `scripts/target_tasks.py` with the following behavior:
- Read `tasks/index.json`.
- For each task, add/overwrite the fields `suite`, `category`, `batch`, `target_subset`, `blocked_by`, `exit_criteria` using keyword heuristics from `title` and `notes`.
- Preserve existing fields (`id`, `title`, `file`, `status`, `priority`, `notes`).
- Write the JSON back with 2-space indentation and a trailing newline.
- Print a summary of tasks per `batch` and any tasks that could not be categorized.

Use this starting implementation:

```python
#!/usr/bin/env python3
"""Add targeting metadata to tasks/index.json for JS/TS 100% compat work."""
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
    "tooling": ["linter", "lint", "script", "hot reload", "bridge", "host", "build"],
}

CATEGORY_KEYWORDS = {
    "harness": ["harness"],
    "measurement": ["measure", "skip list", "report", "conformance runner", "negative-test"],
    "expressions": ["expression", "template literal", "spread", "nullish", "optional chaining", "computed key", "delete", "unary"],
    "statements": ["statement", "for-of", "for-in", "do...while", "switch", "labeled", "with", "while"],
    "functions": ["function", "arguments", "default parameter", "length property", "name property", "arrow"],
    "classes": ["class", "super", "extends", "static field", "private field"],
    "built-ins": ["constructor", "array", "error", "date", "boolean", "number", "string", "regexp", "bigint", "symbol", "parseint", "parsefloat", "json.stringify"],
    "objects": ["object", "property descriptor", "defineProperty", "prototype", "insertion order", "own keys", "hasOwnProperty"],
    "errors": ["error", "throw", "try/catch", "exception", "tdz"],
    "modules": ["module", "import", "export", "moduleResolution"],
    "async": ["promise", "async", "await", "generator", "microtask"],
    "types": ["type", "ts type", "type literal", "type query", "nonprimitive"],
    "jsx": ["jsx", "tsx"],
    "interpreter": ["interpreter", "trampoline", "stack overflow", "scope.rs"],
    "testing": ["unit test", "fixture", "scenario test", "testing strategy"],
    "refactor": ["refactor", "split", "delete dead", "cleanup"],
}

BATCH_OVERRIDES = {
    "253": 0, "91": 0, "250": 0, "97": 0, "344": 0,
    "85": 3, "241": 3, "182": 3, "183": 3, "187": 3, "251": 3,
}

def infer_suite(text: str) -> str:
    lowered = text.lower()
    for suite, kws in SUITE_KEYWORDS.items():
        if any(kw in lowered for kw in kws):
            return suite
    return "runtime"

def infer_category(text: str) -> str:
    lowered = text.lower()
    for category, kws in CATEGORY_KEYWORDS.items():
        if any(kw in lowered for kw in kws):
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
        if category in {"expressions", "built-ins", "objects", "errors"}:
            return 1
        if category in {"statements", "functions", "modules"}:
            return 2
        return 3
    if priority == "P1":
        return 4
    if priority == "P2":
        return 5
    return 6

def target_subset_for(category: str, suite: str) -> str:
    if suite == "harness":
        return "tests/test262/harness/ + tests/typescript/tests/cases/conformance harness integration"
    if category == "measurement":
        return "target/test262_report.md and target/conformance_report.md accuracy"
    if suite in {"test262", "typescript", "both"}:
        return f"tests/{suite if suite != 'both' else 'test262+typescript'} {category}/ subset"
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

def blocked_by_for(tid: str, category: str, batch: int) -> list:
    deps = []
    if batch >= 1 and category not in {"harness", "measurement", "testing", "refactor"}:
        deps.extend(["253", "344"])
    if tid.startswith("290") and tid != "290":
        deps.append("290")
    if tid in {"322a", "322b"}:
        deps.append("322")
    if tid in {"289a", "289b", "289c"}:
        deps.append("289")
    return deps

def main():
    data = json.loads(INDEX.read_text())
    tasks = data["tasks"]
    unclassified = []
    for task in tasks:
        text = f"{task.get('title', '')} {task.get('notes', '')}"
        task["suite"] = infer_suite(text)
        task["category"] = infer_category(text)
        task["batch"] = infer_batch(task)
        task["target_subset"] = target_subset_for(task["category"], task["suite"])
        task["blocked_by"] = blocked_by_for(task["id"], task["category"], task["batch"])
        task["exit_criteria"] = exit_criteria_for(task)
        if task["category"] == "refactor" and task["suite"] == "runtime":
            unclassified.append(task["id"])

    INDEX.write_text(json.dumps(data, indent=2) + "\n")
    print(f"Updated {len(tasks)} tasks.")
    if unclassified:
        print(f"Potentially unclassified tasks: {unclassified}")
    by_batch = {}
    for task in tasks:
        by_batch.setdefault(task["batch"], 0)
        by_batch[task["batch"]] += 1
    for batch in sorted(by_batch):
        print(f"  Batch {batch}: {by_batch[batch]} tasks")

if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/target_tasks.py`

- [ ] **Step 3: Run the script once to preview the transformation**

Run: `python3 scripts/target_tasks.py`

Expected: it prints counts per batch and no errors.

- [ ] **Step 4: Review the diff**

Run: `git diff --stat tasks/index.json`

Expected: ~700 lines changed (one entry per task).

- [ ] **Step 5: Commit**

```bash
git add scripts/target_tasks.py tasks/index.json
git commit -m "feat(tasks): add targeting metadata to every task for 100% compat"
```

### Task 2: Update the compatibility roadmap with batches

**Files:**
- Modify: `docs/js-ts-compatibility-roadmap.md`

- [ ] **Step 1: Insert a "Batched milestones" section before "Testing requirement"**

Add the following Markdown:

```markdown
## Batched milestones

Each batch groups tasks that target a specific slice of the spec suites. A batch closes when every task in it reaches its `exit_criteria` (see `docs/conformance.md`).

| Batch | Theme | Target suite(s) | Key tasks |
|-------|-------|-----------------|-----------|
| 0 | Truthful measurement | harness / test262 / typescript | 253, 91, 250, 97, 344 |
| 1 | Quick syntax / builtin wins | test262 / typescript | 281, 289-290 series, 320-329 |
| 2 | Core semantics | test262 / typescript | 141, 291-295 |
| 3 | Big architecture features | test262 / typescript | 85, 241, 182-187, 251 |
| 4 | P1 correctness | test262 / typescript | 294, 295, and remaining P1 items |
| 5 | P2 medium features | test262 / typescript | RegExp, BigInt, Symbol, Date, additional statements |
| 6 | Host / polish | runtime / tooling | Bridge, hot reload, perf tooling |

Use `python3 scripts/target_tasks.py` to regenerate the per-task metadata after editing tasks.
```

- [ ] **Step 2: Verify the doc renders**

Run: `cat docs/js-ts-compatibility-roadmap.md | head -120`

Expected: the new table appears and the file is valid Markdown.

- [ ] **Step 3: Commit**

```bash
git add docs/js-ts-compatibility-roadmap.md
git commit -m "docs(roadmap): add batched milestones for 100% compat"
```

### Task 3: Add the targeting policy to the conformance doc

**Files:**
- Modify: `docs/conformance.md`

- [ ] **Step 1: Insert a "Targeting policy" section after "Definition of Done for compatibility work"**

Add:

```markdown
## Targeting policy

Every compatibility task must be targeted at a measurable subset of the spec suites:

1. `suite` — `test262`, `typescript`, `both`, `harness`, `runtime`, or `tooling`.
2. `category` — the language area (expressions, statements, functions, classes, built-ins, etc.).
3. `batch` — integer work batch from the roadmap; lower numbers run first.
4. `target_subset` — a concrete path or pattern in `tests/test262` or `tests/typescript` that the task must bring to 100%.
5. `blocked_by` — task IDs that must close first.
6. `exit_criteria` — a verifiable sentence describing the 100% pass condition.

These fields live in `tasks/index.json` and are maintained by `scripts/target_tasks.py`. No compatibility task may be marked complete without a regenerated harness report proving its `target_subset` is at 100% with zero spec skips.
```

- [ ] **Step 2: Verify the doc renders**

Run: `cat docs/conformance.md | head -140`

Expected: the new section appears and the file is valid Markdown.

- [ ] **Step 3: Commit**

```bash
git add docs/conformance.md
git commit -m "docs(conformance): add targeting policy for compat tasks"
```

### Task 4: Add a "Targets" section to the Batch 0 task files

**Files:**
- Modify: `tasks/253-load-real-test262-harness-includes.md`
- Modify: `tasks/91-audit-test262-skip-list.md`
- Modify: `tasks/250-preserve-thrown-values-in-try-catch-throw.md`
- Modify: `tasks/97-precise-negative-test-matching.md`
- Modify: `tasks/344-js-ts-compatibility-harness.md`

- [ ] **Step 1: Append a "Targets" section to each file**

For each file, add at the end:

```markdown
## Targets

- **Suite:** see `tasks/index.json`
- **Batch:** 0
- **Target subset:** see `tasks/index.json`
- **Blocked by:** see `tasks/index.json`
- **Exit criteria:** Relevant harness report reflects truthful pass/fail/skip counts and the target subset reaches 100%.
```

- [ ] **Step 2: Verify each file ends with the section**

Run: `tail -20 tasks/253-load-real-test262-harness-includes.md`

Expected: the "Targets" section is present.

- [ ] **Step 3: Commit**

```bash
git add tasks/253-load-real-test262-harness-includes.md tasks/91-audit-test262-skip-list.md tasks/250-preserve-thrown-values-in-try-catch-throw.md tasks/97-precise-negative-test-matching.md tasks/344-js-ts-compatibility-harness.md
git commit -m "docs(tasks): add Targets section to Batch 0 measurement tasks"
```

### Task 5: Validate the targeting metadata

**Files:**
- Modify: `scripts/target_tasks.py` (if validation gaps are found)

- [ ] **Step 1: Add validation logic to the script**

After the summary, require that every task with `suite` in `{"test262", "typescript", "both"}` has a non-empty `target_subset` and `exit_criteria`. Print any violations and exit non-zero if found.

- [ ] **Step 2: Re-run the script**

Run: `python3 scripts/target_tasks.py`

Expected: zero violations, valid JSON, and a clean per-batch summary.

- [ ] **Step 3: Verify JSON syntax**

Run: `python3 -m json.tool tasks/index.json > /dev/null`

Expected: no output (success).

- [ ] **Step 4: Commit**

```bash
git add scripts/target_tasks.py
git commit -m "feat(tasks): validate targeting metadata in index.json"
```

---

## Self-review checklist

- [ ] Spec coverage: every requirement from `docs/superpowers/specs/2026-07-07-js-ts-compat-targeting.md` has at least one task.
- [ ] Placeholder scan: no "TBD", "TODO", or vague steps remain.
- [ ] Type consistency: field names and enum values match the spec.
