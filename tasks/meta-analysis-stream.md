# Conformance Workflow

`tasks/index.json` is the sole progress record. Do not copy stage counts,
failure counts, completion claims, or active-work notes into documentation.

For a failing stage:

1. Run the stage with `TEST262_DIGEST=1` and group failures by cause.
2. Write one failing Rust unit test for each distinct bug.
3. Make the smallest fix, keeping spec algorithms in JS where possible.
4. Run the unit test, its suite, the stage, formatting, and clippy.
5. Update progress only through the test262 runner workflow.
