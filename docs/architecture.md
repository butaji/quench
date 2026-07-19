# Architecture — Minimum Rust Core, Self-hosted JS Builtins

The runtime is a small Rust core plus a JS-implemented builtin layer.
Everything that can be expressed in JS authored on top of the core spec
ops **is** expressed in JS. JS is ~1/3 the LOC of equivalent Rust and
is far easier to keep spec-faithful, so we exploit that ruthlessly.

## What stays in Rust (the core)

The core is the smallest set that the builtins physically cannot be
written without. Anything outside this list belongs in JS.

```
crates/quench-runtime/src/
├── parser.rs         # oxc → internal AST
├── lower/            # AST lowering
├── ast.rs            # internal AST
├── interpreter.rs    # eval entry points
├── eval/             # tree-walking evaluator
│   ├── ops.rs        # canonical spec abstract operations
│   │                 #   ToPrimitive, ToPropertyKey, ToObject, ToNumber,
│   │                 #   ToString, SameValue, SameValueZero,
│   │                 #   IsCallable, IsConstructor, OrdinaryHasProperty,
│   │                 #   CreateDataPropertyOrThrow, GetIterator,
│   │                 #   IteratorNext, IteratorStep, IteratorClose,
│   │                 #   CreateIterResultObject, native_fn
│   ├── function.rs   # call/construct, this binding, arguments
│   ├── …             # eval nodes only — no spec-op re-implementations
├── env.rs            # lexical environments
├── value/            # Value, Object, Function, NativeFunction, JsError
├── context/          # Context, Realm, globals, CURRENT_CONTEXT
└── builtins/
    ├── core/         # Rust intrinsics backing crate-backed algorithms
    │   ├── regex.rs      # regress-backed RegExp exec / match / split
    │   ├── date.rs       # chrono-backed time math (NaiveDate, Utc)
    │   ├── bigint.rs     # num-bigint-backed arithmetic
    │   ├── json.rs       # serde_json-backed parse / stringify
    │   └── uri.rs        # urlencoding-backed encodeURI / decodeURI
    └── bootstrap.rs  # loads + evaluates builtins/*.js at realm init
```

Plus a single `crates/quench-runtime/builtins/*.js` source tree:

```
crates/quench-runtime/builtins/
├── _intrinsics.js    # private helpers wrapping %ops% object
├── Object.js         # Object static + Object.prototype
├── Function.js       # Function.prototype (call, apply, bind)
├── Array.js          # Array constructor + Array.prototype (map, …)
├── String.js         # String constructor + String.prototype
├── Number.js         # Number constructor + Number.prototype
├── Boolean.js        # Boolean constructor + Boolean.prototype
├── Symbol.js         # Symbol() + Symbol.prototype, registry in Rust
├── Error.js          # Error + NativeError hierarchy
├── Math.js           # Math.* (single Rust call per op)
├── JSON.js           # thin shell over core/json.rs
├── Map.js, Set.js,
├── WeakMap.js, WeakSet.js   # call core ordered-map storage
├── Promise.js        # over core microtask queue
├── RegExp.js         # shell over core/regex.rs
├── Date.js           # shell over core/date.rs
├── BigInt.js         # shell over core/bigint.rs
├── Reflect.js, Proxy.js
├── Iterator.js       # %IteratorPrototype% + Array/String/Map/Set iterators
├── TypedArray.js, ArrayBuffer.js, DataView.js
├── Atomics.js        # thin shell
── decodeURI.js, encodeURI.js   # shell over core/uri.rs
```

## What moves to JS

Anything that is a pure spec algorithm on top of the canonical spec
ops lives in `builtins/*.js`. The whole `Array.prototype.*`,
`String.prototype.*`, `Number.prototype.*`, `Object.keys/assign/freeze`,
`Reflect.*`, `Error.*`, `Map/Set/WeakMap/WeakSet` user surface, the
intrinsic iterator prototypes, `Promise.prototype.*`, etc. No
`NativeFunction` boilerplate per method, no `Rc<RefCell<Object>>`
construction at every callsite, no `JsError::from("TypeError:...")`
protocol duplication. ~3x LOC savings expected per builtin.

## The `%ops%` intrinsic object

At realm init, the Rust core exposes a frozen `%ops%` object whose
properties are the canonical spec abstract operations, implemented once
in `eval/ops.rs` and inserted as `NativeFunction`s. Self-hosted JS
imports it via `import`-style intrinsic access (resolved at parse time
of `builtins/*.js`; never user-visible). Example:

```js
// builtins/Array.js (excerpt)
const { GetIterator, IteratorStep, IteratorValue,
        IsCallable, ToObject, ThrowTypeError } = %ops%;

Array.prototype.map = function (callback, thisArg) {
  const O = ToObject(this);
  const len = O.length >>> 0;
  if (!IsCallable(callback)) throw ThrowTypeError("not a function");
  const A = new Array(len);
  for (let k = 0; k < len; k++) {
    const kPresent = k in O;
    if (kPresent) {
      const kValue = O[k];
      A[k] = callback.call(thisArg, kValue, k, O);
    }
  }
  return A;
};
```

The `%ops%` surface is the **only** Rust↔JS bridge for spec ops. New
op? Add it to `eval/ops.rs` with a test and expose it on `%ops%`. No
second copy in JS.

## Crate-backed builtins — Rust, not JS

These are kept in Rust because the crate already implements the spec
algorithm verbatim (AGENTS.md forbids hand-rolled copies):

| Spec area       | Crate         | Lives in                    |
|-----------------|---------------|-----------------------------|
| RegExp exec     | `regress`     | `builtins/core/regex.rs`    |
| Date math       | `chrono`      | `builtins/core/date.rs`     |
| BigInt          | `num-bigint`  | `builtins/core/bigint.rs`   |
| JSON parse/str  | `serde_json`  | `builtins/core/json.rs`     |
| URI encode/dec  | `urlencoding` | `builtins/core/uri.rs`      |
| Parsing         | `oxc`         | `parser.rs`                 |

These expose a tiny Rust function each (e.g. `RegExpExec(receiver, string) -> ResultMatch`)
and the surrounding `.prototype.*` JS shell lives in `builtins/*.js`.
No "thinly-disguised copy" of any of these under helpers named after
the crate (`chrono_*` that never imports chrono is the bug pattern —
already a finding in the audit, R3 in the queue).

## Realm / bootstrap ordering

`Context::new` builds the core realm (Rust intrinsic prototypes —
`%ObjectPrototype%`, `%FunctionPrototype%`, `%IteratorPrototype%`,
intrinsic error ctors, `%ThrowTypeError%`), then `builtins/bootstrap.rs`
parses `builtins/*.js` in dependency order:

1. `_intrinsics.js` — declares the `%ops%` destructure (resolved by parser).
2. `Object.js` then `Function.js` (foundation — everything inherits).
3. `Error.js`, `Symbol.js`, `Number.js`, `Boolean.js`, `String.js`.
4. `Array.js`, `Iterator.js`.
5. `Map.js`/`Set.js`/`WeakMap.js`/`WeakSet.js`.
6. `Promise.js`, `JSON.js`, `Reflect.js`, `Proxy.js`, `Math.js.
7. `RegExp.js`, `Date.js`, `BigInt.js`, `TypedArray.js`, …
8. URI helpers.

Bootstrap is one-shot per `Realm`; realms reuse the parsed AST cache
across `Context::new` (see `tasks/refactor-plan.md` R5).

## Workflow — same discipline, doubled surface

The `AGENTS.md` "Unit tests, not guesswork" cycle applies to JS
builtins identically:

1. **Reproduce** — add a `#[test]` in the relevant Rust module's
   `mod tests` that exercises the JS builtin through `Context::eval`.
   Mirror `src/builtins/json.rs` tests for the pattern.
2. **Watch it fail** — the test must fail with the same symptom as the
   test262 case. If the bug is in the JS builtin, the unit test still
   lives in Rust: it wraps the failing JS expression.
3. **Fix** — edit `builtins/<name>.js` (or `eval/ops.rs` if a core
   op is wrong). Re-run.
4. **Verify** — `cargo test -p quench-runtime`, then the relevant
   test262 stage.
5. **Leave the test in**.

No `dbg!`/`println!` archaeology in either language. The conformance
gate is unchanged.

## Why this is OK against "No speculative generality"

The core shrinks. Storage enums (`ObjData`, `VTable`) that the audit
flagged as speculative are not needed at all once their would-be
consumers are JS: the JS layer never reaches inside `Object`, only
calls `%ops%`. So the speculative scaffolding from the audit (R2)
becomes deletable *because* of this architecture, not in spite of it.