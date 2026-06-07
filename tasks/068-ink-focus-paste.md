# Task 068: `ink-focus-paste` Example — `useFocus`, `useFocusManager`, `usePaste`

**Priority:** P2-Medium
**Phase:** 6 — Ink Advanced
**Depends on:** 067

## Problem

`useFocus`, `useFocusManager`, and `usePaste` are implemented in the bridge but not validated by a dedicated example.

## Example

```tsx
import { Box, Text, useFocus, useFocusManager, usePaste } from 'ink';
import { useState } from 'react';

function FocusableItem({ label }: { label: string }) {
  const { isFocused } = useFocus();
  return <Text color={isFocused ? 'green' : 'gray'}>{isFocused ? '> ' : '  '}{label}</Text>;
}

export default function App() {
  const [pasted, setPasted] = useState('');
  usePaste({ onPaste: (text: string) => setPasted(text) });

  return (
    <Box flexDirection="column">
      <FocusableItem label="Item 1" />
      <FocusableItem label="Item 2" />
      <Text>Pasted: {pasted || '(none)'}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `useFocus`, `useFocusManager`, `usePaste` exercised
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%