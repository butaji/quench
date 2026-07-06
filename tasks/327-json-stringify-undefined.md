> New low-effort/high-impact win from code review.

# Task 327: Fix JSON.stringify(undefined)

## Status: PENDING

## Problem

`Value::Undefined` and `Value::Null` both serialize as `serialize_unit()`, which serde_json turns into `null`. In JS, `JSON.stringify(undefined)` should return `undefined` (and in arrays it becomes `null`).

## Fix

Special-case `Value::Undefined` at the top-level in `json.rs` before delegating to `JsValueProxy`.

## Acceptance criteria

- [ ] `JSON.stringify(undefined)` returns `undefined`.
- [ ] `JSON.stringify([undefined])` returns `"[null]"`.
- [ ] `JSON.stringify({a: undefined})` returns `"{}"`.
- [ ] Regression test and fixture added.

## Files

- `crates/quench-runtime/src/builtins/mod.rs` (`JsValueProxy::serialize`)
- `crates/quench-runtime/src/builtins/json.rs`

## Effort / impact

- Effort: trivial
- Impact: medium
