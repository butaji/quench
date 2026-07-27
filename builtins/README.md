# Self-Hosted JS Builtins

Builtins are self-hosted in JavaScript on top of `__ops__` (the Rust↔JS bridge).
Each file follows the bootstrap dependency order from `docs/architecture.md`.

## Bootstrap order

1. `_intrinsics.js` — (planned) core intrinsic setup
2. `Object.js` — Object static methods
3. `Function.js` — (planned) Function.prototype methods
4. `Array.js` — Array.isArray + Array.prototype methods
5. (more to follow per architecture.md)

## Pattern

```js
var ops = __ops__;
var IsCallable = ops.IsCallable;
var ToObject = ops.ToObject;

Array.prototype.map = function (callback, thisArg) {
  var O = ToObject(this);
  // ... use IsCallable, ToObject, etc.
};
```

- Methods that would recurse (e.g., sort calling itself) stay native.
- Each file is loaded via `include_str!` in `bootstrap.rs`.
- Tests live in `crates/quench-runtime/src/builtins/bootstrap.rs`.
