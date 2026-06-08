# Task 137: Support React Fragment Shorthand `<></>`

**Priority:** P0-Critical
**Phase:** 12 — Real-World Validation
**Depends on:** 132

## Problem

The `../tui1` example uses React Fragment shorthand syntax:

```tsx
const renderInput = () => {
  // ...
  return (
    <>
      <Text color={C.fgMid}>{before}</Text>
      <Text backgroundColor={C.fgBright} color={C.bg}>{char}</Text>
      <Text color={C.fgMid}>{after}</Text>
    </>
  );
};
```

The `oxc_parser` / `oxc_transformer` pipeline may or may not transform `<></>` into `React.createElement(React.Fragment, null, ...)`. If not, the generated JS will contain syntax the rquickjs parser cannot handle.

## Verification

Check generated JS bundle for `<>...</>` syntax. If present, the transform pipeline needs updating.

## Acceptance Criteria

- [ ] `<></>` is transformed to `React.createElement(React.Fragment, null, ...)` in JS bundle
- [ ] `React.Fragment` is defined in the React shim
- [ ] `../tui1` example renders fragments correctly in all 3 environments
- [ ] Parity harness 100% match for fragment-containing examples
