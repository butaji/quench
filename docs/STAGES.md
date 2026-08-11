# Test262 stages

This file defines the conformance stages for `quench`. Each stage is one
numbered test262 directory that must reach 100% passing before the next stage
begins. There are no skip policies and no checkpoints: each stage runs through
the canonical `quench-test262` runner against the pinned `tests/test262` tree,
and observable behavior is verified at execution time, never worked around.

This is a definition document only — it is not a progress ledger. Verify stages
with the relevant commands and test262 runs at execution time.

## Execution order

Stages are grouped by domain and revisited when needed if a later stage exposes a
semantic gap.

0. `harness` (stage 0) — the test262 harness self-tests that validate the
   assertion and helper libraries the runner composes.
1. `language` (stages 1–28) — core ECMAScript syntax and semantics.
2. `built-ins` (stages 29–92) — intrinsic objects and their methods.
3. `annexB` (stages 93–94) — Annex B web-compat extensions (built on core language).
4. `intl402` (stages 95–113) — ECMA-402 `Intl` (built on Number, String, Date, BigInt, Array).

`staging` holds proposal work and is excluded from stable conformance coverage;
it must not be silently counted as stable progress.

## `harness` domain

The test262 harness self-tests in `test/harness`. They exercise `assert`,
`sta`, `asyncHelpers`, and the support helpers that the runner composes into
every case; they must pass through the exact harness composition, never by
overriding harness behavior.

- Stage 0: `test/harness`

## `language` domain

Core language behavior. Parse, scoping, and evaluation semantics.

- Stage 1: `language/arguments-object`
- Stage 2: `language/asi`
- Stage 3: `language/block-scope`
- Stage 4: `language/comments`
- Stage 5: `language/computed-property-names`
- Stage 6: `language/destructuring`
- Stage 7: `language/directive-prologue`
- Stage 8: `language/eval-code`
- Stage 9: `language/export`
- Stage 10: `language/expressions`
- Stage 11: `language/function-code`
- Stage 12: `language/future-reserved-words`
- Stage 13: `language/global-code`
- Stage 14: `language/identifier-resolution`
- Stage 15: `language/identifiers`
- Stage 16: `language/import`
- Stage 17: `language/keywords`
- Stage 18: `language/line-terminators`
- Stage 19: `language/literals`
- Stage 20: `language/module-code`
- Stage 21: `language/punctuators`
- Stage 22: `language/reserved-words`
- Stage 23: `language/rest-parameters`
- Stage 24: `language/source-text`
- Stage 25: `language/statementList`
- Stage 26: `language/statements`
- Stage 27: `language/types`
- Stage 28: `language/white-space`

## `built-ins` domain

Intrinsic objects, constructors, prototypes, and global functions.

- Stage 29: `built-ins/AbstractModuleSource`
- Stage 30: `built-ins/AggregateError`
- Stage 31: `built-ins/Array`
- Stage 32: `built-ins/ArrayBuffer`
- Stage 33: `built-ins/ArrayIteratorPrototype`
- Stage 34: `built-ins/AsyncDisposableStack`
- Stage 35: `built-ins/AsyncFromSyncIteratorPrototype`
- Stage 36: `built-ins/AsyncFunction`
- Stage 37: `built-ins/AsyncGeneratorFunction`
- Stage 38: `built-ins/AsyncGeneratorPrototype`
- Stage 39: `built-ins/AsyncIteratorPrototype`
- Stage 40: `built-ins/Atomics`
- Stage 41: `built-ins/BigInt`
- Stage 42: `built-ins/Boolean`
- Stage 43: `built-ins/DataView`
- Stage 44: `built-ins/Date`
- Stage 45: `built-ins/DisposableStack`
- Stage 46: `built-ins/Error`
- Stage 47: `built-ins/FinalizationRegistry`
- Stage 48: `built-ins/Function`
- Stage 49: `built-ins/GeneratorFunction`
- Stage 50: `built-ins/GeneratorPrototype`
- Stage 51: `built-ins/Infinity`
- Stage 52: `built-ins/Iterator`
- Stage 53: `built-ins/JSON`
- Stage 54: `built-ins/Map`
- Stage 55: `built-ins/MapIteratorPrototype`
- Stage 56: `built-ins/Math`
- Stage 57: `built-ins/NaN`
- Stage 58: `built-ins/NativeErrors`
- Stage 59: `built-ins/Number`
- Stage 60: `built-ins/Object`
- Stage 61: `built-ins/Promise`
- Stage 62: `built-ins/Proxy`
- Stage 63: `built-ins/Reflect`
- Stage 64: `built-ins/RegExp`
- Stage 65: `built-ins/RegExpStringIteratorPrototype`
- Stage 66: `built-ins/Set`
- Stage 67: `built-ins/SetIteratorPrototype`
- Stage 68: `built-ins/ShadowRealm`
- Stage 69: `built-ins/SharedArrayBuffer`
- Stage 70: `built-ins/String`
- Stage 71: `built-ins/StringIteratorPrototype`
- Stage 72: `built-ins/SuppressedError`
- Stage 73: `built-ins/Symbol`
- Stage 74: `built-ins/Temporal`
- Stage 75: `built-ins/ThrowTypeError`
- Stage 76: `built-ins/TypedArray`
- Stage 77: `built-ins/TypedArrayConstructors`
- Stage 78: `built-ins/Uint8Array`
- Stage 79: `built-ins/WeakMap`
- Stage 80: `built-ins/WeakRef`
- Stage 81: `built-ins/WeakSet`
- Stage 82: `built-ins/decodeURI`
- Stage 83: `built-ins/decodeURIComponent`
- Stage 84: `built-ins/encodeURI`
- Stage 85: `built-ins/encodeURIComponent`
- Stage 86: `built-ins/eval`
- Stage 87: `built-ins/global`
- Stage 88: `built-ins/isFinite`
- Stage 89: `built-ins/isNaN`
- Stage 90: `built-ins/parseFloat`
- Stage 91: `built-ins/parseInt`
- Stage 92: `built-ins/undefined`

## `annexB` domain

Annex B web-compat extensions. These depend on the core `language` and
`built-ins` domains.

- Stage 93: `annexB/built-ins`
- Stage 94: `annexB/language`

## `intl402` domain

ECMA-402 `Intl` behavior. Depends on the relevant `built-ins` primitives
(Number, String, Date, BigInt, Array).

- Stage 95: `intl402/Array`
- Stage 96: `intl402/BigInt`
- Stage 97: `intl402/Collator`
- Stage 98: `intl402/Date`
- Stage 99: `intl402/DateTimeFormat`
- Stage 100: `intl402/DisplayNames`
- Stage 101: `intl402/DurationFormat`
- Stage 102: `intl402/FallbackSymbol`
- Stage 103: `intl402/Intl`
- Stage 104: `intl402/ListFormat`
- Stage 105: `intl402/Locale`
- Stage 106: `intl402/Number`
- Stage 107: `intl402/NumberFormat`
- Stage 108: `intl402/PluralRules`
- Stage 109: `intl402/RelativeTimeFormat`
- Stage 110: `intl402/Segmenter`
- Stage 111: `intl402/String`
- Stage 112: `intl402/Temporal`
- Stage 113: `intl402/TypedArray`

## Excluded from stable coverage

- `staging` — proposal work, not stable conformance.
