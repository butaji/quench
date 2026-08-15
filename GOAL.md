# Goal: stages 16, 19, and 20

Bring test262 stages 16, 19, and 20 to 100% passing through the canonical
runner: import, literals, and module-code. Preserve UTF-16 lone surrogates,
module/linking behavior, parse and completion semantics, and exact error
ordering. Fix the canonical parser/evaluator/runtime path rather than adding
test-specific behavior; do not alter `tests/test262` or the harness. Re-run
these stages and all earlier completed regressions after each fix. Finish with
zero failures, clean format/clippy checks, and committed verified changes.
