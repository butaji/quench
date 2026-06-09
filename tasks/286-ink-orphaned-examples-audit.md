# Task 286: Audit and Track Orphaned Existing Ink Examples

**Priority:** P0-Critical
**Phase:** 24 — Real-World Example Audit
**Depends on:** 285

## Problem

There are **92 existing Ink examples** in `examples/` that have no corresponding task in `tasks/index.json`. These examples already exercise important Ink features (layout, styling, hooks, input handling, etc.) and must be tracked for parity testing.

## Orphaned Examples (92 total)

Layout and styling:
- `ink-absolute`, `ink-align-self`, `ink-aligned`, `ink-all-border-styles`, `ink-all-text-styles`, `ink-background-color`, `ink-border-color`, `ink-bordered`, `ink-box`, `ink-dimensions`, `ink-display`, `ink-flex-basis`, `ink-flex-reverse`, `ink-flex-shrink`, `ink-gaps`, `ink-justify-space`, `ink-layout`, `ink-margin`, `ink-min-max-size`, `ink-nested-layouts`, `ink-padding`, `ink-partial-border`, `ink-relative`, `ink-spacer`, `ink-split-pane`, `ink-wrap`, `ink-z-index`

Components and rendering:
- `ink-box`, `ink-conditional`, `ink-conditional-rendering`, `ink-context`, `ink-counter`, `ink-cursor`, `ink-custom-render`, `ink-dynamic`, `ink-dynamic-children`, `ink-enter-submit`, `ink-focus`, `ink-focus-cycle`, `ink-focus-manager`, `ink-focus-next`, `ink-form-checkbox`, `ink-form-layout`, `ink-form-switch`, `ink-fragment`, `ink-fragment-advanced`, `ink-hooks`, `ink-import`, `ink-input`, `ink-input-hook`, `ink-inverse`, `ink-key-events`, `ink-list`, `ink-list-advanced`, `ink-menu`, `ink-menu-advanced`, `ink-mouse-events`, `ink-multi-select`, `ink-multiple-colors`, `ink-newline`, `ink-overflow`, `ink-paste`, `ink-progress`, `ink-progress-bar`, `ink-raw`, `ink-ref`, `ink-rerender`, `ink-static`, `ink-static-color`, `ink-status-bar`, `ink-stderr`, `ink-stdin`, `ink-stdin-advanced`, `ink-stdout`, `ink-switch`, `ink-table`, `ink-table-advanced`, `ink-text-props`, `ink-text-styling`, `ink-todo`, `ink-transform`, `ink-type-assertions`, `ink-uncontrolled-input`, `ink-use-animation`, `ink-use-app`, `ink-use-box-metrics`, `ink-use-callback`, `ink-use-effect`, `ink-use-memo`, `ink-window-size`

Real-world:
- `ink-chat`, `ink-combined-hooks`

## Steps

1. Run `scripts/parity.sh --env all` against all 92 orphaned examples.
2. For each example that passes, create a tracking task in `tasks/index.json` or bulk-add them.
3. For each example that fails, file a bug-fix task and fix the underlying compile-path or bridge issue.
4. Update `tasks/index.json` coverage_gaps to remove Ink features covered by these examples.


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] All 92 orphaned examples are tracked in `tasks/index.json`.
- [ ] Parity harness passes for all 92 orphaned examples with 100% match across deno, `runts dev`, and `runts build`.
- [ ] `tasks/index.json` coverage_gaps is updated to reflect newly covered features.
- [ ] `EXECUTE.md` is updated with the new task count.
