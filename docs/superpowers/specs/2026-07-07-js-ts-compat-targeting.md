# JS/TS Compatibility Targeting Design

**Goal:** Every doc and task in the project is explicitly aimed at reaching 100% compatibility with the `tests/test262` and `tests/typescript` submodule suites, organized into small, measurable batches.

**Current state:** The project already has the harness (`docs/conformance.md`), a roadmap (`docs/js-ts-compatibility-roadmap.md`), and a task registry (`tasks/index.json`). However, most tasks only carry `priority` and free-form `notes`; there is no uniform way to see which test suite, category, batch, or subset a task targets. The result is a large pile of P0 work without a clear, measurable order of attack.

## Approaches considered

### A. Keep the current structure, just rewrite docs
Rewrite `docs/js-ts-compatibility-roadmap.md` with a single linear list of batches and manually link tasks. Leave `tasks/index.json` unchanged.

- **Pros:** No schema change; fast.
- **Cons:** Tasks remain untargeted; the doc and the registry diverge; impossible to generate a batch report automatically.

### B. Add targeting metadata to every task and generate reports (recommended)
Extend `tasks/index.json` with `suite`, `category`, `batch`, `target_subset`, `blocked_by`, and `exit_criteria`. Update the roadmap and conformance docs to reference the batch taxonomy. Provide a small script that validates the metadata and prints a batch report.

- **Pros:** All tasks are explicitly targeted; docs and registry stay in sync; batch progress can be tracked mechanically; easy to split work into granular batches.
- **Cons:** One-time cost to annotate ~90 tasks; requires discipline to keep metadata up to date.

### C. Replace `tasks/index.json` with a full project-management backend
Move tasks into a database or structured YAML with dependencies, effort estimates, Gantt-style scheduling, etc.

- **Pros:** Very powerful.
- **Cons:** Massive over-engineering for a small team; conflicts with the existing simple JSON registry; not necessary to reach 100% compat.

**Recommendation:** Approach B. It gives the project a targeting system without adding weight.

## Design

### Targeting schema (one entry per task in `tasks/index.json`)

| Field | Type | Meaning |
|-------|------|---------|
| `suite` | string enum | Primary test suite this task advances: `test262`, `typescript`, `both`, `runtime`, `harness`, `tooling`. |
| `category` | string enum | Semantic area: `harness`, `measurement`, `expressions`, `statements`, `functions`, `classes`, `built-ins`, `objects`, `errors`, `modules`, `async`, `types`, `jsx`, `interpreter`, `testing`, `refactor`. |
| `batch` | integer | Work batch. Lower numbers are prerequisites or quick wins; higher numbers are larger features or polish. |
| `target_subset` | string | Specific submodule path or pattern the task must bring to 100%, e.g. `tests/test262/test/language/expressions/`. |
| `blocked_by` | array of task IDs | Tasks that must close before this one can realistically close. |
| `exit_criteria` | string | Verifiable sentence: "test262 language/expressions subset passes at 100% with zero spec skips." |

`title`, `file`, `status`, `priority`, and `notes` remain unchanged.

### Batch taxonomy

| Batch | Theme | Suite emphasis | Typical exit criteria |
|-------|-------|----------------|----------------------|
| 0 | Truthful measurement | `harness`, `test262`, `typescript` | Real harness helpers loaded; skip list audited; reports reflect actual state. |
| 1 | Quick syntax / builtin wins | `test262`, `typescript` | Small subsets (expressions, basic constructors, primitive methods) reach 100%. |
| 2 | Core semantics | `test262`, `typescript` | Scope, hoisting, TDZ, strict mode, arguments, typeof, global object subsets reach 100%. |
| 3 | Big architecture features | `test262`, `typescript` | Trampoline interpreter, ES modules, classes, Promises/async unblock whole suite areas. |
| 4 | P1 correctness | `test262`, `typescript` | Property descriptors, prototype chains, built-in object semantics reach 100%. |
| 5 | P2 medium features | `test262`, `typescript` | RegExp, BigInt, Symbol, Date, additional control-flow statements reach 100%. |
| 6 | P3 host / polish | `runtime`, `tooling` | Host bridge, hot reload, perf tooling are done and do not regress conformance. |

A task's `batch` is derived from its priority and category:
- P0 harness/measurement → 0
- P0 quick wins (small expression/builtin fixes) → 1
- P0 core semantics → 2
- P0 big architecture → 3
- P1 → 4
- P2 → 5
- P3 / runtime / tooling → 6

### Doc updates

1. `docs/js-ts-compatibility-roadmap.md` — add a "Batched milestones" section that maps each batch to its tasks and expected conformance delta.
2. `docs/conformance.md` — add a "Targeting policy" section that requires every compatibility task to carry the new schema fields and links to the roadmap batches.

### Validation

A small script `scripts/target_tasks.py` will:
- Read `tasks/index.json`.
- Infer or validate the new fields.
- Print a batch report: tasks per batch, per suite, per category, blocked tasks, and missing `target_subset`/`exit_criteria` entries.
- Exit non-zero if any compatibility task lacks required fields after the transition.

The script is idempotent and can be rerun as tasks are added or closed.

## Assumptions

- The existing task registry stays the single source of truth; no new database or project-management tool is introduced.
- `target_subset` may be coarse for large tasks (e.g. "full test262 language/statements") and refined as work progresses.
- Runtime/performance/tooling tasks that do not directly move a spec suite still get a `suite: runtime`/`tooling` entry so they are not orphaned, but their `exit_criteria` focus on regression prevention.
