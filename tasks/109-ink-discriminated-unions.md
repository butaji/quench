# Task 109: `ink-discriminated-unions` Example — Discriminated Unions, Exhaustive Checks

**Priority:** P1-High
**Phase:** 11 — Type System Deep Coverage
**Depends on:** 078

## Problem

Discriminated unions (tagged unions with a shared discriminant property) are one of TypeScript's most powerful patterns. Combined with exhaustive switch checks, they enable robust state modeling. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-discriminated-unions/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

type Event =
  | { type: 'click'; x: number; y: number }
  | { type: 'keypress'; key: string }
  | { type: 'resize'; width: number; height: number };

function handleEvent(e: Event): string {
  switch (e.type) {
    case 'click':
      return `Click at (${e.x}, ${e.y})`;
    case 'keypress':
      return `Key: ${e.key}`;
    case 'resize':
      return `Resized to ${e.width}x${e.height}`;
    default:
      const _exhaustive: never = e;
      return _exhaustive;
  }
}

type Shape =
  | { kind: 'circle'; radius: number }
  | { kind: 'rect'; width: number; height: number };

function area(s: Shape): number {
  switch (s.kind) {
    case 'circle': return Math.PI * s.radius * s.radius;
    case 'rect': return s.width * s.height;
    default:
      const _e: never = s;
      return _e;
  }
}

const events: Event[] = [
  { type: 'click', x: 10, y: 20 },
  { type: 'keypress', key: 'Enter' },
];

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>{handleEvent(events[0])}</Text>
      <Text>{handleEvent(events[1])}</Text>
      <Text>Circle area: {area({ kind: 'circle', radius: 5 }).toFixed(2)}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-discriminated-unions/`
- [ ] Uses discriminated union with `type`/`kind` property
- [ ] Uses exhaustive switch with `never` check
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path erases discriminated unions without runtime impact
- [ ] Parity harness passes with 100% match in all 3 environments
