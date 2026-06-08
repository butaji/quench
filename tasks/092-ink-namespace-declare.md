# Task 092: `ink-namespace-declare` Example — `namespace`, `declare`

**Priority:** P2-Medium
**Phase:** 10 — Extended TS/TSX Coverage
**Depends on:** 078

## Problem

`namespace` (formerly `module`) and `declare` are TypeScript declaration patterns for organizing types and ambient declarations. No existing Ink example exercises these.

## Ink Example

```tsx
// examples/ink-namespace-declare/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

namespace AppConfig {
  export const version = '1.0.0';
  export interface Theme {
    name: string;
  }
}

declare const BUILD_DATE: string;
const buildDate = typeof BUILD_DATE !== 'undefined' ? BUILD_DATE : 'dev';

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Version: {AppConfig.version}</Text>
      <Text>Build: {buildDate}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [x] Example exists at `examples/ink-namespace-declare/`
- [x] Uses `namespace` declaration with exported values
- [x] Uses `declare` for ambient values
- [x] Renders identically in deno and `runts dev` (100% output match)
- [x] Compile path erases `namespace` and `declare` without runtime impact
- [x] Parity harness passes with 100% match in all 3 environments
