# Task 287: Runtime optimization roadmap

## Status: IN PROGRESS

## Goal

Track the architecture / code / tasks review findings and ensure the runtime moves toward the best possible performance with less code.

## Outputs

- `docs/runtime-optimization-roadmap.md` — ranked P0/P1/P2/P3 recommendations.
- New quick-win tasks: 281, 282, 283, 284, 285, 286.

## Principles

1. Less hot-path allocation.
2. No `HashMap` in the hot path.
3. Explicit state instead of thread-locals.
4. Unified value model.
5. One correctness fix, one focused test.

## Next steps

Complete the quick-win tasks (281–286) before larger HIR work. Update this task and the roadmap doc as findings change.

## Verification

- `docs/runtime-optimization-roadmap.md` is current.
- `tasks/index.json` reflects optimization priorities.
