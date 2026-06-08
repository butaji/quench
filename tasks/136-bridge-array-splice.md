# Task 136: Verify `Array.prototype.splice` in rquickjs

**Priority:** P1-High
**Phase:** 12 — Real-World Validation
**Depends on:** 132

## Problem

The `../tui1` example uses `Array.prototype.splice` for cursor-based text insertion:

```tsx
inputBuffer.splice(pos, 0, _str);
```

`splice` is a core Array method. It may already work in rquickjs (QuickJS implements ES2020). This task is to **verify** it works and document any edge cases.

## Verification

```js
// Test in rquickjs context
const arr = [1, 2, 3];
arr.splice(1, 0, 'a');
// Expected: [1, 'a', 2, 3]
```

## Acceptance Criteria

- [ ] `arr.splice(start, deleteCount, ...items)` works in rquickjs
- [ ] Returns removed elements array
- [ ] Mutates original array
- [ ] `../tui1` example does not fail on `inputBuffer.splice(...)`
