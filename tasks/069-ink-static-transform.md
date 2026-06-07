# Task 069: `ink-static-transform` Example — `Static`, `Transform`, `Newline`, `Spacer`

**Priority:** P2-Medium
**Phase:** 6 — Ink Advanced
**Depends on:** 068

## Problem

`Static`, `Transform`, `Newline`, and `Spacer` are implemented but not all have dedicated examples.

## Example

```tsx
import { Box, Text, Static, Transform, Newline, Spacer } from 'ink';

export default function App() {
  return (
    <Box flexDirection="column">
      <Static items={['A', 'B', 'C']}>
        {(item) => <Text key={item}>Static: {item}</Text>}
      </Static>
      <Transform transform={(output) => output.toUpperCase()}>
        <Text>transformed</Text>
      </Transform>
      <Newline />
      <Text>Before spacer</Text>
      <Spacer />
      <Text>After spacer</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `Static`, `Transform`, `Newline`, `Spacer` all exercised
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%