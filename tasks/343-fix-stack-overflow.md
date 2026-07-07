> **Closed duplicate of Task 85.**

# Task 343: Fix Stack Overflow in Examples

## Status: CLOSED (duplicate of Task 85)

The canonical fix for recursive-interpreter stack overflow is Task 85 (trampoline interpreter). The false stack-overflow errors seen under parallel tests are fixed by Task 338 (thread-local depth counter). No separate implementation is required here.

## Exact Fix

See Task 85 for the exact trampoline-interpreter implementation and Task 338 for the exact thread-local depth-counter change.

## Targets

- **Suite:** `runtime`
- **Batch:** 1
- **Target subset:** n/a (interpreter infrastructure)
- **Blocked by:** 85, 338
- **Exit criteria:** Closed as duplicate; no separate work.
