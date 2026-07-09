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
    "330": {"suite": "tooling", "category": "testing", "priority": "P0", "batch": 0},
    "331": {"suite": "tooling", "category": "testing"},
    "332": {"suite": "tooling", "category": "testing"},
    "334": {"suite": "harness", "category": "measurement", "batch": 0},
    "335": {"suite": "both", "category": "objects", "batch": 1, "priority": "P0"},
    "336": {"suite": "both", "category": "built-ins", "batch": 2},
    "337": {"suite": "tooling", "category": "testing", "batch": 6},
    "338": {"suite": "runtime", "category": "interpreter", "priority": "P0"},
    "342": {"suite": "tooling", "category": "testing"},
    "351": {"suite": "tooling", "category": "testing", "batch": 0},
    "352": {"suite": "tooling", "category": "testing", "batch": 0},
    "353": {"suite": "tooling", "category": "testing", "batch": 0},
    "344": {"priority": "P0", "suite": "both", "category": "harness", "batch": 0},
    # Coverage milestone tasks must cite concrete matrix paths, not broad categories.
    "309": {"target_subset": "tests/test262/test/language/expressions", "exit_criteria": "All test262 language/expressions/ files are active and pass at 100% with zero spec skips."},
    "310": {"target_subset": "tests/test262/test/language/statements", "exit_criteria": "All test262 language/statements/ files are active and pass at 100% with zero spec skips."},
    "311": {"target_subset": "tests/test262/test/language/function-code; tests/test262/test/language/arguments-object; tests/test262/test/language/rest-parameters; tests/test262/test/language/statements/function; tests/test262/test/language/expressions/function", "exit_criteria": "All test262 function-related areas are active and pass at 100% with zero spec skips."},
    "312": {"target_subset": "tests/test262/test/built-ins/Object; tests/test262/test/built-ins/Reflect", "exit_criteria": "All test262 built-ins/Object/ and built-ins/Reflect/ files are active and pass at 100% with zero spec skips."},
    "313": {"target_subset": "tests/test262/test/built-ins/Array; tests/test262/test/built-ins/String; tests/test262/test/built-ins/Number; tests/test262/test/built-ins/Boolean; tests/test262/test/built-ins/Date; tests/test262/test/built-ins/Error", "exit_criteria": "All core built-in suites (Array, String, Number, Boolean, Date, Error) are active and pass at 100% with zero spec skips."},
    "314": {"target_subset": "tests/test262/test/language/statements/class; tests/test262/test/language/expressions/class; tests/test262/test/language/computed-property-names/class", "exit_criteria": "All test262 class syntax areas are active and pass at 100% with zero spec skips."},
    "315": {"target_subset": "tests/test262/test/language/module-code; tests/test262/test/language/import; tests/test262/test/language/export", "exit_criteria": "All test262 module-code, import, and export areas are active and pass at 100% with zero spec skips."},
    "316": {"target_subset": "tests/test262/test/built-ins/Error; tests/test262/test/built-ins/NativeErrors", "exit_criteria": "All test262 built-ins/Error/ and built-ins/NativeErrors/ files are active and pass at 100% with zero spec skips."},
    "317": {"target_subset": "tests/test262/test/built-ins/Promise; tests/test262/test/language/expressions/async-function; tests/test262/test/language/statements/async-function; tests/test262/test/built-ins/AsyncFunction; tests/test262/test/built-ins/GeneratorFunction; tests/test262/test/built-ins/GeneratorPrototype", "exit_criteria": "All Promise, async-function, and generator areas are active and pass at 100% with zero spec skips."},
    "318": {"target_subset": "tests/typescript/tests/cases/conformance", "exit_criteria": "The full TypeScript conformance suite passes at 100% with zero spec skips."},
    "253": {
        "suite": "harness",
        "category": "harness",
        "target_subset": "tests/test262/harness/ helpers loaded into the runner",
        "exit_criteria": "Real test262 harness includes (assert.js, sta.js, compareArray.js, etc.) load and run; no test is skipped solely because a helper is stubbed.",
    },
    "97": {
        "suite": "harness",
        "category": "measurement",
        "target_subset": "target/test262_report.md and target/conformance_report.md accuracy",
        "exit_criteria": "test262 negative tests match by expected error type and phase with inheritance support, and the harness report is regenerated.",
    },
    "304": {
        "target_subset": "tests/test262/test/language/function-code",
        "exit_criteria": "Function-scope closure environment correctly inherits from the function closure; test262 language/function-code/ subset passes at 100% with zero spec skips.",
    },
    "292": {
        "target_subset": "tests/test262/test/language/statements/variable; tests/test262/test/language/statements/let; tests/test262/test/language/statements/const",
        "exit_criteria": "test262 language/statements/variable/, language/statements/let/, and language/statements/const/ subsets pass at 100% with zero spec skips.",
    },
    "339": {
        "target_subset": "tests/test262/test/language/statements/variable",
        "exit_criteria": "test262 language/statements/variable/ subset passes at 100% with zero spec skips.",
    },
    "340": {
        "target_subset": "tests/test262/test/language/statements/let; tests/test262/test/language/statements/const",
        "exit_criteria": "test262 language/statements/let/ and language/statements/const/ subsets pass at 100% with zero spec skips.",
    },
    "350": {
        "target_subset": "tests/test262/test/language/statements/for-of; tests/test262/test/language/block-scope",
        "exit_criteria": "test262 language/statements/for-of/ and language/block-scope/ subsets pass at 100% with zero spec skips.",
    },
    "305": {
        "target_subset": "tests/test262/test/language/rest-parameters",
        "exit_criteria": "test262 language/rest-parameters/ subset passes at 100% with zero spec skips.",
    },
    "307": {
        "target_subset": "tests/test262/test/built-ins/Array/prototype/flat; tests/test262/test/built-ins/Array/prototype/flatMap",
        "exit_criteria": "test262 built-ins/Array/prototype/flat/ and flatMap/ subsets pass at 100% with zero spec skips.",
    },
    # Runtime / tooling tasks that were mis-inferred as compatibility tasks.
    "85": {"suite": "runtime", "category": "interpreter", "priority": "P0"},
    "286": {"suite": "runtime", "category": "interpreter"},
    "335": {"suite": "runtime", "category": "interpreter", "priority": "P0"},
    "282": {"suite": "runtime", "category": "interpreter"},
    "325": {"suite": "tooling", "category": "refactor"},
    "329": {"suite": "tooling", "category": "refactor"},
    # Harness / measurement tasks.
    "91": {"suite": "harness", "category": "measurement", "batch": 0},
    "280": {"suite": "harness", "category": "testing"},
    "344": {"suite": "harness", "category": "measurement", "batch": 0, "priority": "P0"},
    "82": {"suite": "harness", "category": "measurement", "batch": 7},
    # Granular compatibility tasks with concrete matrix paths.
    "250": {"target_subset": "tests/test262/test/built-ins/Error; tests/test262/test/built-ins/NativeErrors", "exit_criteria": "test262 built-ins/Error/ and built-ins/NativeErrors/ subsets pass at 100% with zero spec skips."},
    "289": {"target_subset": "tests/test262/test/built-ins/Array; tests/test262/test/built-ins/Error; tests/test262/test/built-ins/Date", "exit_criteria": "test262 built-ins/Array/, built-ins/Error/, and built-ins/Date/ constructor subsets pass at 100% with zero spec skips."},
    "289a": {"target_subset": "tests/test262/test/built-ins/Array", "exit_criteria": "test262 built-ins/Array/ subset passes at 100% with zero spec skips."},
    "289b": {"target_subset": "tests/test262/test/built-ins/Error", "exit_criteria": "test262 built-ins/Error/ subset passes at 100% with zero spec skips."},
    "289c": {"target_subset": "tests/test262/test/built-ins/Date", "exit_criteria": "test262 built-ins/Date/ subset passes at 100% with zero spec skips."},
    "290": {"target_subset": "tests/test262/test/language/literals/template; tests/test262/test/language/computed-property-names; tests/test262/test/language/expressions/array; tests/test262/test/language/expressions/coalesce; tests/test262/test/language/expressions/delete; tests/test262/test/language/expressions/unary-plus; tests/test262/test/language/statements/for-of", "exit_criteria": "All expression quick-win areas are active and pass at 100% with zero spec skips."},
    "290a": {"target_subset": "tests/test262/test/language/literals/template", "exit_criteria": "test262 language/literals/template/ subset passes at 100% with zero spec skips."},
    "290b": {"target_subset": "tests/test262/test/language/computed-property-names", "exit_criteria": "test262 language/computed-property-names/ subset passes at 100% with zero spec skips."},
    "290c": {"target_subset": "tests/test262/test/language/expressions/array", "exit_criteria": "test262 language/expressions/array/ subset passes at 100% with zero spec skips."},
    "290d": {"target_subset": "tests/test262/test/language/expressions/coalesce", "exit_criteria": "test262 language/expressions/coalesce/ subset passes at 100% with zero spec skips."},
    "290e": {"target_subset": "tests/test262/test/language/expressions/delete", "exit_criteria": "test262 language/expressions/delete/ subset passes at 100% with zero spec skips."},
    "290f": {"target_subset": "tests/test262/test/language/expressions/unary-plus", "exit_criteria": "test262 language/expressions/unary-plus/ subset passes at 100% with zero spec skips."},
    "290g": {"target_subset": "tests/test262/test/language/statements/for-of", "exit_criteria": "test262 language/statements/for-of/ subset passes at 100% with zero spec skips."},
    "291": {"target_subset": "tests/test262/test/language/expressions/typeof", "exit_criteria": "test262 language/expressions/typeof/ subset passes at 100% with zero spec skips."},
    "293": {"target_subset": "tests/test262/test/language/arguments-object", "exit_criteria": "test262 language/arguments-object/ subset passes at 100% with zero spec skips."},
    "320": {"target_subset": "tests/test262/test/built-ins/Object", "exit_criteria": "test262 built-ins/Object/ subset passes at 100% with zero spec skips."},
    "321": {"target_subset": "tests/test262/test/built-ins/Boolean; tests/test262/test/built-ins/Number", "exit_criteria": "test262 built-ins/Boolean/ and built-ins/Number/ subsets pass at 100% with zero spec skips."},
    "322": {"target_subset": "tests/test262/test/built-ins/Object/prototype; tests/test262/test/built-ins/Function/prototype", "exit_criteria": "test262 built-ins/Object/prototype/ and built-ins/Function/prototype/ subsets pass at 100% with zero spec skips."},
    "322a": {"target_subset": "tests/test262/test/built-ins/Object/prototype", "exit_criteria": "test262 built-ins/Object/prototype/ subset passes at 100% with zero spec skips."},
    "322b": {"target_subset": "tests/test262/test/built-ins/Function/prototype", "exit_criteria": "test262 built-ins/Function/prototype/ subset passes at 100% with zero spec skips."},
    "323": {"target_subset": "tests/test262/test/built-ins/Object", "exit_criteria": "test262 built-ins/Object/ subset passes at 100% with zero spec skips."},
    "324": {"target_subset": "tests/test262/test/built-ins/parseInt; tests/test262/test/built-ins/parseFloat", "exit_criteria": "test262 built-ins/parseInt/ and built-ins/parseFloat/ subsets pass at 100% with zero spec skips."},
    "336": {"target_subset": "tests/test262/test/built-ins/Array", "exit_criteria": "test262 built-ins/Array/ subset passes at 100% with zero spec skips when using ecosystem crates."},
    "105": {"target_subset": "tests/test262/test/built-ins/Error/prototype", "exit_criteria": "test262 built-ins/Error/prototype/ subset passes at 100% with zero spec skips."},
    "112": {"target_subset": "tests/test262/test/built-ins/Error; tests/test262/test/built-ins/NativeErrors", "exit_criteria": "test262 built-ins/Error/ and built-ins/NativeErrors/ subsets pass at 100% with zero spec skips."},
    "117": {"target_subset": "tests/test262/test/language/function-code", "exit_criteria": "test262 language/function-code/ subset passes at 100% with zero spec skips."},
    "119": {"target_subset": "tests/test262/test/built-ins/Function", "exit_criteria": "test262 built-ins/Function/ subset passes at 100% with zero spec skips."},
    "124": {"target_subset": "tests/test262/test/built-ins/Function", "exit_criteria": "test262 built-ins/Function/ subset passes at 100% with zero spec skips."},
    "132": {"target_subset": "tests/test262/test/built-ins/Error/name", "exit_criteria": "test262 built-ins/Error/name/ subset passes at 100% with zero spec skips."},
    "147": {"target_subset": "tests/test262/test/built-ins/Array/prototype/flat", "exit_criteria": "test262 built-ins/Array/prototype/flat/ subset passes at 100% with zero spec skips."},
    "191": {"target_subset": "tests/test262/test/built-ins/String/prototype/toLocaleLowerCase; tests/test262/test/built-ins/String/prototype/toLocaleUpperCase", "exit_criteria": "test262 String locale case subsets pass at 100% with zero spec skips."},
    "239": {"target_subset": "tests/test262/test/built-ins/Boolean; tests/test262/test/built-ins/String", "exit_criteria": "test262 built-ins/Boolean/ and built-ins/String/ subsets pass at 100% with zero spec skips."},
    "283": {"target_subset": "tests/test262/test/built-ins/String/prototype", "exit_criteria": "test262 built-ins/String/prototype/ subset passes at 100% with zero spec skips."},
    "284": {"target_subset": "tests/test262/test/built-ins/Array/prototype", "exit_criteria": "test262 built-ins/Array/prototype/ subset passes at 100% with zero spec skips."},
    "326": {"target_subset": "tests/test262/test/built-ins/String/prototype/length", "exit_criteria": "test262 built-ins/String/prototype/length/ subset passes at 100% with zero spec skips."},
    "327": {"target_subset": "tests/test262/test/built-ins/JSON", "exit_criteria": "test262 built-ins/JSON/ subset passes at 100% with zero spec skips."},
    "328": {"target_subset": "tests/test262/test/language/statements/for-of", "exit_criteria": "test262 language/statements/for-of/ subset passes at 100% with zero spec skips."},
    "341": {"target_subset": "tests/test262/test/language/expressions/new", "exit_criteria": "test262 language/expressions/new/ subset passes at 100% with zero spec skips."},
    "182": {"target_subset": "tests/test262/test/language/statements/class; tests/test262/test/language/expressions/class", "exit_criteria": "test262 class declaration and expression subsets pass at 100% with zero spec skips."},
    "183": {"target_subset": "tests/test262/test/language/statements/class/elements", "exit_criteria": "test262 language/statements/class/elements/ subset passes at 100% with zero spec skips."},
    "187": {"target_subset": "tests/test262/test/language/statements/class/subclass; tests/test262/test/language/expressions/class/subclass", "exit_criteria": "test262 class subclass subsets pass at 100% with zero spec skips."},
    "241": {"target_subset": "tests/test262/test/language/module-code; tests/test262/test/language/import; tests/test262/test/language/export", "exit_criteria": "test262 module-code, import, and export subsets pass at 100% with zero spec skips."},
    "251": {"target_subset": "tests/test262/test/built-ins/Promise", "exit_criteria": "test262 built-ins/Promise/ subset passes at 100% with zero spec skips."},
    "141": {"target_subset": "tests/test262/test/language/expressions/arrow-function", "exit_criteria": "test262 language/expressions/arrow-function/ subset passes at 100% with zero spec skips."},
    "294": {"target_subset": "tests/test262/test/built-ins/Object/defineProperty; tests/test262/test/built-ins/Object/getOwnPropertyDescriptor", "exit_criteria": "test262 property-descriptor subsets pass at 100% with zero spec skips."},
    "295": {"target_subset": "tests/test262/test/built-ins/global", "exit_criteria": "test262 built-ins/global/ subset passes at 100% with zero spec skips."},
    "296": {"target_subset": "tests/test262/test; tests/typescript/tests/cases/conformance", "exit_criteria": "Both full conformance suites pass at 100% with zero spec skips."},
    "87": {"suite": "tooling", "category": "refactor"},
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
    # Core VM/runtime foundation is Batch 1: build the proper runtime before
    # the 100% language-compat crunch.
    "85": 1,
    "285": 1,
    "286": 1,
    "308": 1,
    "333": 1,
    "335": 1,
    "338": 1,
    "343": 1,
    # Advanced runtime / HIR / performance work stays until after 100%
    # conformance (Batch 8).
    "88": 8,
    "264": 8,
    "287": 8,
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
    # Guard-rail: completed compat tasks must also have a ## Verification section in the
    # markdown file, so background processes cannot mark tasks complete on code alone.
    violations = []
    generic_targets = []
    for task in tasks:
        if task["suite"] in {"test262", "typescript", "both"}:
            target_subset = task.get("target_subset", "")
            if not target_subset or target_subset.startswith("n/"):
                violations.append((task["id"], "missing target_subset"))
            elif " subset" in target_subset or not target_subset.startswith("tests/"):
                # Vague targets like "expressions subset" make it easy to avoid work.
                msg = f"generic target_subset: {target_subset[:50]}"
                if task.get("status") == "completed":
                    violations.append((task["id"], msg))
                else:
                    generic_targets.append((task["id"], msg))
            if not task.get("exit_criteria"):
                violations.append((task["id"], "missing exit_criteria"))
            if task.get("status") == "completed":
                file_text = Path(task["file"]).read_text()
                if not re.search(r"^##\s*Verification", file_text, re.MULTILINE | re.IGNORECASE):
                    violations.append((task["id"], "completed compat task missing ## Verification section"))

    if generic_targets:
        print("\nGeneric target_subset warnings (pending tasks must be tightened before completion):")
        for tid, reason in generic_targets:
            print(f"  {tid}: {reason}")

    if violations:
        print("\nTargeting violations:")
        for tid, reason in violations:
            print(f"  {tid}: {reason}")
        return 1

    print("\nAll compatibility tasks are targeted.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
