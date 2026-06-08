# Task 228: `ink-intl-number` Example — `Intl.NumberFormat`

**Priority:** P2-Medium
**Phase:** 20 — Runtime API Deep Coverage
**Depends on:** 227

## Problem

`Intl.NumberFormat` formats numbers for specific locales. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-intl-number/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const num = 1234567.89;
  const formatter = new Intl.NumberFormat('de-DE', {
    style: 'currency',
    currency: 'EUR',
  });
  const formatted = formatter.format(num);

  return (
    <Box flexDirection="column">
      <Text>Number: {formatted}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-intl-number/`
- [ ] Uses `Intl.NumberFormat` with currency style
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for Intl APIs
- [ ] Parity harness passes with 100% match in all 3 environments
