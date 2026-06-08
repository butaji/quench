# Task 139: Handle `/** @jsxImportSource react */` Pragma

**Priority:** P1-High
**Phase:** 12 — Real-World Validation
**Depends on:** 132

## Problem

The `../tui1` example starts with a JSX pragma comment:

```tsx
/** @jsxImportSource react */
import React, { useState, useEffect, useRef } from "react";
```

This tells the TypeScript/JSX compiler to use `react` as the JSX factory source. Our `oxc_transformer` pipeline may:
1. Strip the comment harmlessly
2. Respect it and change the JSX factory (which should still work since React is the target)
3. Fail to parse it

## Verification

Check generated JS bundle to see if the pragma causes any transformation issues.

## Acceptance Criteria

- [ ] `/** @jsxImportSource react */` is handled correctly by `oxc_transformer`
- [ ] Generated JS bundle still uses `React.createElement` or equivalent
- [ ] `../tui1` example JSX renders correctly despite the pragma
- [ ] No transformation errors in the JS bundle
