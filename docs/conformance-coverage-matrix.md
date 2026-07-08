> **Authoritative coverage backlog.** A compatibility task is not done until the area it claims is listed here as *Active & passing*.

# Conformance Coverage Matrix

This document enumerates every spec area that must be active and passing before Quench reaches 100% JS/TS conformance.
It is the source of truth for `target_subset` values in task files and for the active harness subset.

## Legend

| Status | Meaning |
|---|---|
| `- [ ]` | Not yet in the active harness subset. |
| `- [/]` | In the active subset but still failing or spec-skipped. |
| `- [x]` | Active, passing at 100% with zero spec skips. |

## How to use this matrix

1. When you start a compatibility task, pick the concrete area(s) from this matrix and put the exact path(s) in the task `## Targets` section.
2. Add the area to the active harness subset in `crates/quench-runtime/tests/test262.rs` or `crates/quench-runtime/tests/conformance.rs`.
3. Run the harness, get the area to 100% pass / 0 spec skips, regenerate `target/test262_report.json` or `target/conformance_report.json`.
4. Update this matrix to `- [x]` for that area.
5. Only then may the task `## Status` be changed to `COMPLETED`.

---

## test262 coverage

Total files in `tests/test262/test/`: **53,683**. Every directory below must reach `- [x]`.

### annexB (1,086 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/test262/test/annexB/built-ins` | 241 | |
| - [ ] | `tests/test262/test/annexB/language` | 845 | |

### built-ins (23,671 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/test262/test/built-ins/AbstractModuleSource` | 8 | |
| - [ ] | `tests/test262/test/built-ins/AggregateError` | 25 | |
| - [ ] | `tests/test262/test/built-ins/Array` | 3,081 | |
| - [ ] | `tests/test262/test/built-ins/ArrayBuffer` | 221 | |
| - [ ] | `tests/test262/test/built-ins/ArrayIteratorPrototype` | 27 | |
| - [ ] | `tests/test262/test/built-ins/AsyncDisposableStack` | 104 | |
| - [ ] | `tests/test262/test/built-ins/AsyncFromSyncIteratorPrototype` | 38 | |
| - [ ] | `tests/test262/test/built-ins/AsyncFunction` | 18 | |
| - [ ] | `tests/test262/test/built-ins/AsyncGeneratorFunction` | 23 | |
| - [ ] | `tests/test262/test/built-ins/AsyncGeneratorPrototype` | 48 | |
| - [ ] | `tests/test262/test/built-ins/AsyncIteratorPrototype` | 13 | |
| - [ ] | `tests/test262/test/built-ins/Atomics` | 389 | |
| - [ ] | `tests/test262/test/built-ins/BigInt` | 77 | |
| - [ ] | `tests/test262/test/built-ins/Boolean` | 51 | |
| - [ ] | `tests/test262/test/built-ins/DataView` | 561 | |
| - [ ] | `tests/test262/test/built-ins/Date` | 594 | |
| - [ ] | `tests/test262/test/built-ins/DisposableStack` | 93 | |
| - [ ] | `tests/test262/test/built-ins/Error` | 93 | |
| - [ ] | `tests/test262/test/built-ins/FinalizationRegistry` | 47 | |
| - [ ] | `tests/test262/test/built-ins/Function` | 509 | |
| - [ ] | `tests/test262/test/built-ins/GeneratorFunction` | 23 | |
| - [ ] | `tests/test262/test/built-ins/GeneratorPrototype` | 61 | |
| - [ ] | `tests/test262/test/built-ins/Infinity` | 6 | |
| - [ ] | `tests/test262/test/built-ins/Iterator` | 514 | |
| - [ ] | `tests/test262/test/built-ins/JSON` | 165 | |
| - [ ] | `tests/test262/test/built-ins/Map` | 204 | |
| - [ ] | `tests/test262/test/built-ins/MapIteratorPrototype` | 11 | |
| - [ ] | `tests/test262/test/built-ins/Math` | 327 | |
| - [ ] | `tests/test262/test/built-ins/NaN` | 6 | |
| - [ ] | `tests/test262/test/built-ins/NativeErrors` | 94 | |
| - [ ] | `tests/test262/test/built-ins/Number` | 340 | |
| - [ ] | `tests/test262/test/built-ins/Object` | 3,411 | |
| - [ ] | `tests/test262/test/built-ins/Promise` | 729 | |
| - [ ] | `tests/test262/test/built-ins/Proxy` | 311 | |
| - [ ] | `tests/test262/test/built-ins/Reflect` | 153 | |
| - [ ] | `tests/test262/test/built-ins/RegExp` | 1,879 | |
| - [ ] | `tests/test262/test/built-ins/RegExpStringIteratorPrototype` | 17 | |
| - [ ] | `tests/test262/test/built-ins/Set` | 383 | |
| - [ ] | `tests/test262/test/built-ins/SetIteratorPrototype` | 11 | |
| - [ ] | `tests/test262/test/built-ins/ShadowRealm` | 67 | |
| - [ ] | `tests/test262/test/built-ins/SharedArrayBuffer` | 104 | |
| - [ ] | `tests/test262/test/built-ins/String` | 1,223 | |
| - [ ] | `tests/test262/test/built-ins/StringIteratorPrototype` | 7 | |
| - [ ] | `tests/test262/test/built-ins/SuppressedError` | 22 | |
| - [ ] | `tests/test262/test/built-ins/Symbol` | 98 | |
| - [ ] | `tests/test262/test/built-ins/Temporal` | 4,603 | |
| - [ ] | `tests/test262/test/built-ins/ThrowTypeError` | 14 | |
| - [ ] | `tests/test262/test/built-ins/TypedArray` | 1,446 | |
| - [ ] | `tests/test262/test/built-ins/TypedArrayConstructors` | 738 | |
| - [ ] | `tests/test262/test/built-ins/Uint8Array` | 70 | |
| - [ ] | `tests/test262/test/built-ins/WeakMap` | 141 | |
| - [ ] | `tests/test262/test/built-ins/WeakRef` | 29 | |
| - [ ] | `tests/test262/test/built-ins/WeakSet` | 85 | |
| - [ ] | `tests/test262/test/built-ins/decodeURI` | 55 | |
| - [ ] | `tests/test262/test/built-ins/decodeURIComponent` | 56 | |
| - [ ] | `tests/test262/test/built-ins/encodeURI` | 31 | |
| - [ ] | `tests/test262/test/built-ins/encodeURIComponent` | 31 | |
| - [ ] | `tests/test262/test/built-ins/eval` | 10 | |
| - [ ] | `tests/test262/test/built-ins/global` | 29 | |
| - [ ] | `tests/test262/test/built-ins/isFinite` | 15 | |
| - [ ] | `tests/test262/test/built-ins/isNaN` | 15 | |
| - [ ] | `tests/test262/test/built-ins/parseFloat` | 54 | |
| - [ ] | `tests/test262/test/built-ins/parseInt` | 55 | |
| - [ ] | `tests/test262/test/built-ins/undefined` | 8 | |

### harness (116 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/test262/test/harness` | 116 | |

### intl402 (3,341 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/test262/test/intl402/Array` | 2 | |
| - [ ] | `tests/test262/test/intl402/BigInt` | 11 | |
| - [ ] | `tests/test262/test/intl402/Collator` | 65 | |
| - [ ] | `tests/test262/test/intl402/Date` | 12 | |
| - [ ] | `tests/test262/test/intl402/DateTimeFormat` | 244 | |
| - [ ] | `tests/test262/test/intl402/DisplayNames` | 57 | |
| - [ ] | `tests/test262/test/intl402/DurationFormat` | 110 | |
| - [ ] | `tests/test262/test/intl402/FallbackSymbol` | 2 | |
| - [ ] | `tests/test262/test/intl402/Intl` | 66 | |
| - [ ] | `tests/test262/test/intl402/ListFormat` | 81 | |
| - [ ] | `tests/test262/test/intl402/Locale` | 152 | |
| - [ ] | `tests/test262/test/intl402/Number` | 7 | |
| - [ ] | `tests/test262/test/intl402/NumberFormat` | 249 | |
| - [ ] | `tests/test262/test/intl402/PluralRules` | 53 | |
| - [ ] | `tests/test262/test/intl402/RelativeTimeFormat` | 80 | |
| - [ ] | `tests/test262/test/intl402/Segmenter` | 79 | |
| - [ ] | `tests/test262/test/intl402/String` | 19 | |
| - [ ] | `tests/test262/test/intl402/Temporal` | 2,029 | |
| - [ ] | `tests/test262/test/intl402/TypedArray` | 1 | |

### language (23,979 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/test262/test/language/arguments-object` | 263 | |
| - [ ] | `tests/test262/test/language/asi` | 102 | |
| - [ ] | `tests/test262/test/language/block-scope` | 145 | |
| - [ ] | `tests/test262/test/language/comments` | 52 | |
| - [ ] | `tests/test262/test/language/computed-property-names` | 48 | |
| - [ ] | `tests/test262/test/language/destructuring` | 19 | |
| - [ ] | `tests/test262/test/language/directive-prologue` | 62 | |
| - [ ] | `tests/test262/test/language/eval-code` | 347 | |
| - [ ] | `tests/test262/test/language/export` | 3 | |
| - [ ] | `tests/test262/test/language/expressions` | 11,158 | |
| - [ ] | `tests/test262/test/language/function-code` | 217 | |
| - [ ] | `tests/test262/test/language/future-reserved-words` | 55 | |
| - [ ] | `tests/test262/test/language/global-code` | 42 | |
| - [ ] | `tests/test262/test/language/identifier-resolution` | 14 | |
| - [ ] | `tests/test262/test/language/identifiers` | 268 | |
| - [ ] | `tests/test262/test/language/import` | 182 | |
| - [ ] | `tests/test262/test/language/keywords` | 25 | |
| - [ ] | `tests/test262/test/language/line-terminators` | 41 | |
| - [ ] | `tests/test262/test/language/literals` | 534 | |
| - [ ] | `tests/test262/test/language/module-code` | 755 | |
| - [ ] | `tests/test262/test/language/punctuators` | 11 | |
| - [ ] | `tests/test262/test/language/reserved-words` | 27 | |
| - [ ] | `tests/test262/test/language/rest-parameters` | 11 | |
| - [ ] | `tests/test262/test/language/source-text` | 1 | |
| - [ ] | `tests/test262/test/language/statementList` | 80 | |
| - [ ] | `tests/test262/test/language/statements` | 9,337 | |
| - [ ] | `tests/test262/test/language/types` | 113 | |
| - [ ] | `tests/test262/test/language/white-space` | 67 | |

### staging (1,490 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/test262/test/staging/Temporal` | 2 | |
| - [ ] | `tests/test262/test/staging/Uint8Array` | 1 | |
| - [ ] | `tests/test262/test/staging/built-ins` | 8 | |
| - [ ] | `tests/test262/test/staging/decorators` | 3 | |
| - [ ] | `tests/test262/test/staging/explicit-resource-management` | 53 | |
| - [ ] | `tests/test262/test/staging/set-methods` | 3 | |
| - [ ] | `tests/test262/test/staging/sm` | 1,409 | |
| - [ ] | `tests/test262/test/staging/source-phase-imports` | 3 | |
| - [ ] | `tests/test262/test/staging/top-level-await` | 4 | |

---

## TypeScript conformance coverage

Total files in `tests/typescript/tests/cases/conformance/`: **5,695**. Every directory below must reach `- [x]`.

### conformance (5,695 files)

| Status | Path | Files | Notes |
|---|---|---:|---|
| - [ ] | `tests/typescript/tests/cases/conformance/Symbols` | 8 | |
| - [ ] | `tests/typescript/tests/cases/conformance/additionalChecks` | 1 | |
| - [ ] | `tests/typescript/tests/cases/conformance/ambient` | 22 | |
| - [ ] | `tests/typescript/tests/cases/conformance/async` | 185 | |
| - [ ] | `tests/typescript/tests/cases/conformance/asyncGenerators` | 3 | |
| - [ ] | `tests/typescript/tests/cases/conformance/classes` | 466 | |
| - [ ] | `tests/typescript/tests/cases/conformance/constEnums` | 9 | |
| - [ ] | `tests/typescript/tests/cases/conformance/controlFlow` | 56 | |
| - [ ] | `tests/typescript/tests/cases/conformance/declarationEmit` | 23 | |
| - [ ] | `tests/typescript/tests/cases/conformance/decorators` | 88 | |
| - [ ] | `tests/typescript/tests/cases/conformance/directives` | 5 | |
| - [ ] | `tests/typescript/tests/cases/conformance/dynamicImport` | 71 | |
| - [ ] | `tests/typescript/tests/cases/conformance/emitter` | 13 | |
| - [ ] | `tests/typescript/tests/cases/conformance/enums` | 14 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2016` | 1 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2017` | 12 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2018` | 4 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2019` | 13 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2020` | 15 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2021` | 12 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2022` | 7 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2023` | 2 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2024` | 3 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es2025` | 4 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es5` | 1 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es6` | 1,045 | |
| - [ ] | `tests/typescript/tests/cases/conformance/es7` | 45 | |
| - [ ] | `tests/typescript/tests/cases/conformance/esDecorators` | 110 | |
| - [ ] | `tests/typescript/tests/cases/conformance/esnext` | 2 | |
| - [ ] | `tests/typescript/tests/cases/conformance/expressions` | 376 | |
| - [ ] | `tests/typescript/tests/cases/conformance/externalModules` | 227 | |
| - [ ] | `tests/typescript/tests/cases/conformance/functions` | 18 | |
| - [ ] | `tests/typescript/tests/cases/conformance/generators` | 15 | |
| - [ ] | `tests/typescript/tests/cases/conformance/importAssertion` | 5 | |
| - [ ] | `tests/typescript/tests/cases/conformance/importAttributes` | 11 | |
| - [ ] | `tests/typescript/tests/cases/conformance/importDefer` | 17 | |
| - [ ] | `tests/typescript/tests/cases/conformance/interfaces` | 66 | |
| - [ ] | `tests/typescript/tests/cases/conformance/internalModules` | 76 | |
| - [ ] | `tests/typescript/tests/cases/conformance/jsdoc` | 341 | |
| - [ ] | `tests/typescript/tests/cases/conformance/jsx` | 4 | |
| - [ ] | `tests/typescript/tests/cases/conformance/moduleResolution` | 51 | |
| - [ ] | `tests/typescript/tests/cases/conformance/node` | 94 | |
| - [ ] | `tests/typescript/tests/cases/conformance/nonjsExtensions` | 5 | |
| - [ ] | `tests/typescript/tests/cases/conformance/override` | 31 | |
| - [ ] | `tests/typescript/tests/cases/conformance/parser` | 819 | |
| - [ ] | `tests/typescript/tests/cases/conformance/pedantic` | 2 | |
| - [ ] | `tests/typescript/tests/cases/conformance/references` | 15 | |
| - [ ] | `tests/typescript/tests/cases/conformance/salsa` | 191 | |
| - [ ] | `tests/typescript/tests/cases/conformance/scanner` | 35 | |
| - [ ] | `tests/typescript/tests/cases/conformance/statements` | 203 | |
| - [ ] | `tests/typescript/tests/cases/conformance/types` | 842 | |
| - [ ] | `tests/typescript/tests/cases/conformance/typings` | 9 | |

## Current active subset

The harness currently runs only the paths hard-coded in:
- `crates/quench-runtime/tests/test262.rs`: `language/expressions/modulus`, `language/expressions/addition`, `language/expressions/arrow-function`, `built-ins/Array/length`, `language/statements/debugger/empty`.
- `crates/quench-runtime/tests/conformance.rs`: `expressions/` (all), plus a 100-case sanity slice and a few specific cases.

The active subset must grow until it equals the full matrix above.

