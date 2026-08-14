# Goal: stage 40

Bring test262 stage 40, `built-ins/Atomics`, to 100% passing through the
canonical runner. Implement the complete Atomics family with exact coercion,
validation, shared/resizable-buffer behavior, index evaluation order, notify
completion values, wait/async-wait behavior, and error classification. Fix
shared canonical semantics; never shim the harness or edit test262. Re-run
stage 40 and earlier regressions after each fix, then run format/clippy and
commit the verified result.
