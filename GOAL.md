# Goal: stage 20 residuals

Bring test262 stage 20 (`language/module-code`) to 100% through the canonical
runner, starting from the current 597/599 baseline. Fix only shared module
linking, evaluation, completion, and error-ordering semantics; do not alter
test262 or the harness. Re-run stage 20 and earlier regressions, verify strict
format/clippy, and commit the verified result.
