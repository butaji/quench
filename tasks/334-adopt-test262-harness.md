> **Minimum custom code.** Reuse the standard test262 runner instead of maintaining a custom one.

# Task 334: Adopt test262-harness

## Goal

Replace the custom Rust test262 runner with `test262-harness` (npm) + a thin wrapper script, so conformance testing is driven by a community-maintained tool and our custom code only provides the engine binary.

## Rationale

test262-harness already handles:
- Test discovery and frontmatter parsing
- Harness helper injection (`assert.js`, `sta.js`, etc.)
- Per-test timeout and error classification
- Reporter output (json, markdown, etc.)

Writing and maintaining these in Rust duplicates effort and drifts from the spec suite format.

## Acceptance criteria

- [ ] `npm install -g test262-harness` (or local `package.json`) is documented.
- [ ] A single shell command runs the full test262 suite against `quench-runtime`.
- [ ] The custom Rust runner is removed or reduced to an optional wrapper.
- [ ] CI/regeneration script uses the new harness.
- [ ] No loss of existing test coverage.

## Targets

- **Suite:** `test262`
- **Batch:** 0
- **Target subset:** `tests/test262/` full suite
- **Blocked by:** 253, 344
- **Exit criteria:** test262 passes through `test262-harness`, reports are generated, and the old custom runner is deleted.
