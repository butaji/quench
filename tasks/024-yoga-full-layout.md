# Task 024: Yoga Full Layout Calculation

## Goal
Trigger `calculate_layout` on root and propagate computed layouts to all nodes.

## Acceptance Criteria
- [ ] On `__ink_commit()`, Yoga root `calculate_layout(width, height)` called.
- [ ] Every `InkNode.yoga.get_layout()` yields valid `left`, `top`, `width`, `height`.
- [ ] Layout survives across commits (incremental updates).
- [ ] Integration test: 20-node tree layout completes in < 2 ms.

## Dependencies
- Task 023, Task 022, Task 005

## SPEC Reference
§3 Rust Modules (ink/); §5 Event Loop
