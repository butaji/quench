# Task 078: Final Coverage Audit — Complete TS/TSX Feature Matrix

**Priority:** P1-High
**Phase:** 9 — Final Audit
**Depends on:** 077

## Problem

There is no single document mapping every TS/TSX/React/Ink feature to its status.

## Work

### 1. Build the feature matrix

Create `docs/SUPPORTED_SUBSET.md` with a comprehensive table:

| Feature | Parser | HIR | Codegen | Example | Tests | Task |
|---------|--------|-----|---------|---------|-------|------|
| `let`/`const`/`var` | ✅ | ✅ | ✅ | ✅ | ✅ | — |
| `for` / `while` / `do-while` | ✅ | ✅ | ✅ | ✅ | ✅ | 042 |
| `switch` | ✅ | ✅ | ✅ | ✅ | ✅ | 042 |
| `try`/`catch`/`finally`/`throw` | ✅ | ✅ | ✅ | ✅ | ✅ | 043 |
| `for-in` / `for-of` | ✅ | ✅ | ✅ | ✅ | ✅ | 044 |
| Destructuring (defaults, rest) | ✅ | ✅ | ✅ | ✅ | ✅ | 045 |
| Object/array spread | ✅ | ✅ | ✅ | ✅ | ✅ | 046 |
| Template literals | ✅ | ✅ | ✅ | ✅ | ✅ | 047 |
| Getters/setters/computed keys | ✅ | ✅ | ✅ | ✅ | ✅ | 048 |
| `??` | ✅ | ✅ | ✅ | ✅ | ✅ | 049 |
| `?.` | ✅ | ✅ | ✅ | ✅ | ✅ | 068, 049 |
| `typeof` / `instanceof` | ✅ | ✅ | ✅ | ✅ | ✅ | 050 |
| Compound assign / bitwise | ✅ | ✅ | ✅ | ✅ | ✅ | 051 |
| `async` / `await` | ✅ | ✅ | ✅ | ✅ | ✅ | 052 |
| Generators `function*` | ✅ | ✅ | ✅ | ✅ | ✅ | 072, 053 |
| Default/rest params | ✅ | ✅ | ✅ | ✅ | ✅ | 054 |
| Classes / `extends` / `super` | ✅ | ✅ | ✅ | ✅ | ✅ | 055 |
| Static methods / private fields | ✅ | ✅ | ✅ | ✅ | ✅ | 071, 056 |
| Getters/setters in classes | ✅ | ✅ | ✅ | ✅ | ✅ | 057 |
| Module exports | ✅ | ✅ | ✅ | ✅ | ✅ | 058 |
| Dynamic imports | ✅ | ✅ | ⚠️ | ✅ | ✅ | 073, 059 |
| `useReducer` / `useContext` / `memo` | N/A | N/A | N/A | ✅ | ✅ | 060 |
| `useMemo` / `useCallback` / `forwardRef` | N/A | N/A | N/A | ✅ | ✅ | 060 |
| `useAnimation` | N/A | N/A | N/A | ✅ | ✅ | 062 |
| `measureElement` / `useBoxMetrics` | N/A | N/A | N/A | ✅ | ✅ | 063 |
| `useFocus` / `useFocusManager` / `usePaste` | N/A | N/A | N/A | ✅ | ✅ | 064 |
| `Static` / `Transform` / `Newline` / `Spacer` | N/A | N/A | N/A | ✅ | ✅ | 065 |
| JSX spread / dynamic / fragments | ✅ | ✅ | ✅ | ✅ | ✅ | 061 |
| Enums / `as` / `satisfies` | ✅ | ✅ | ✅ | ✅ | ✅ | 070, 066 |
| Generics / mapped types | ✅ | ⚠️ | ⚠️ | ✅ | ✅ | 067 |

### 2. Set v1.0 targets

- **P0 (must work):** Core language + JSX + basic React + basic Ink — 100% coverage
- **P1 (should work):** Advanced control flow, async, spread, nullish ops, classes
- **P2 (nice to have):** Generators, enums, decorators
- **P3 (out of scope):** `eval`, `with`, dynamic imports in compile path

### 3. Update `tasks/index.json`

Add `coverage_matrix` field.

## Acceptance Criteria

- [ ] Coverage matrix exists in `docs/SUPPORTED_SUBSET.md`
- [ ] Every TS/TSX/React/Ink feature is mapped
- [ ] Every ❌ or ⚠️ has a linked task number
- [ ] Matrix is accurate as of the audit date
