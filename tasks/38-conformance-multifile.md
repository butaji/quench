# Task 38: Support multi-file conformance cases with `// @filename:`

## Goal

Make the conformance harness correctly handle TypeScript test cases that contain multiple logical files separated by `// @filename:` markers.

## Pareto & reuse note

- Prefer existing crates, the Rust standard library, and OS features over custom code.
- Follow the 80/20 rule: handle the common two- or three-file case first.
- Defer edge cases, but document them in this task or spawn a follow-up task so they are not lost.

## TDD & testing note

- Follow the red-green-refactor cycle: write a failing unit test first, then the minimal code to pass it, then refactor.
- Add a regression test for every bug fix and edge case covered by this task.
- Keep tests in `crates/quench-runtime/tests/` and run `cargo test -p quench-runtime` before marking work done.

## Background

TypeScript splits a single `.ts` test file into units in `TestCaseParser.makeUnitsFromTest` (`tests/typescript/src/harness/harnessIO.ts:1232–1384`). Everything before the first `// @filename:` belongs to the test file's own name, and each subsequent marker starts a new unit. The baseline then contains a `//// [<name>.js]` section for each unit.

Example:

```ts
// @filename: a.ts
export const x = 1;

// @filename: b.ts
import { x } from "./a";
console.log(x);
```

Baseline:

```
//// [a.js]
"use strict";
exports.x = 1;

//// [b.js]
"use strict";
const a_1 = require("./a");
console.log(a_1.x);
```

## Files

- `crates/quench-runtime/tests/conformance.rs`

## Steps

1. Add a `split_units(source: &str, default_name: &str) -> Vec<(String, String)>` helper.
2. If a case has multiple units, evaluate all emitted JS sections in the same `Context` so imports/requires between units resolve.
3. Update `extract_js_from_baseline` to return a map from filename to JS section, then concatenate in unit order.
4. Add a unit test that parses a synthetic multi-file case and verifies both units are extracted.

## Boundaries

- Only modify test harness code.
- Do not modify `tests/typescript/`.

## Acceptance criteria

- A multi-file conformance case with `// @filename:` no longer fails due to missing sections.
- The harness reports the case as passed if all units execute without error.

## Verification

```bash
cargo test -p quench-runtime --test conformance
```
