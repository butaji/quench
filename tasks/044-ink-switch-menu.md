# Task 044: `ink-switch-menu` Example — `switch`, `case`, `default`, Fallthrough

**Priority:** P1-High  
**Phase:** 6 — Control Flow  
**Depends on:** 043

## Problem

Only 1 example uses `switch`. Codegen converts to `if/else` chain but lacks end-to-end validation.

## Example

```tsx
import { Box, Text, useInput, useState } from 'ink';

export default function App() {
  const [selected, setSelected] = useState(0);
  const items = ['Home', 'Settings', 'About', 'Quit'];

  useInput((_, key) => {
    if (key.upArrow) setSelected(s => Math.max(0, s - 1));
    if (key.downArrow) setSelected(s => Math.min(items.length - 1, s + 1));
  });

  let color: string;
  switch (selected) {
    case 0: color = 'green'; break;
    case 1: color = 'yellow'; break;
    case 2: color = 'blue'; break;
    case 3: color = 'red'; break;
    default: color = 'white'; break;
  }

  return (
    <Box flexDirection="column">
      <Text color={color}>> {items[selected]}</Text>
      {items.map((item, idx) => (
        <Text key={idx} dimColor={idx !== selected}>
          {idx === selected ? '> ' : '  '}{item}
        </Text>
      ))}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `switch` with `case`/`default` produces compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%
