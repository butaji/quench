# Conformance Workflow

`tasks/index.json` is the sole progress record. Do not copy stage counts,
failure counts, completion claims, or active-work notes into documentation.

For a failing stage:

1. Run the stage with `TEST262_DIGEST=1` and group failures by cause.
2. Write one failing Rust unit test for each distinct bug.
3. Make the smallest fix on the canonical spec-op path
   (`eval/ops.rs` + `value/*`). Spec algorithms go in JS only once the
   JS builtins layer is live (decision R22 — it is currently dormant).
4. Run the unit test, its suite, the stage, formatting, and clippy.
5. Update progress only through the test262 runner workflow. If the
   runner reports a stage below 100%, it is not done — reconcile
   `tasks/index.json` statuses through the runner (R24), never by hand.
