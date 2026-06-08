# Task 148: `ink-error-subclasses` Example — `TypeError`, `RangeError`, `ReferenceError`

**Priority:** P1-High
**Phase:** 12 — Runtime API Completion
**Depends on:** 147

## Problem

JavaScript error subclasses (`TypeError`, `RangeError`, `ReferenceError`) are used for specific error types. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-error-subclasses/tui/app.tsx
import React, { useState } from 'react';
import { Box, Text } from 'ink';

function validateAge(age: number): void {
  if (typeof age !== 'number') {
    throw new TypeError('Age must be a number');
  }
  if (age < 0 || age > 150) {
    throw new RangeError('Age must be between 0 and 150');
  }
}

export default function App() {
  const [results, setResults] = useState<string[]>([]);

  const tests = [30, -5, 'hello' as unknown as number];
  tests.forEach(age => {
    try {
      validateAge(age);
      setResults(prev => [...prev, `Age ${age}: valid`]);
    } catch (err) {
      if (err instanceof TypeError) {
        setResults(prev => [...prev, `TypeError: ${(err as Error).message}`]);
      } else if (err instanceof RangeError) {
        setResults(prev => [...prev, `RangeError: ${(err as Error).message}`]);
      } else {
        setResults(prev => [...prev, `Error: ${(err as Error).message}`]);
      }
    }
  });

  return (
    <Box flexDirection="column">
      {results.map((r, i) => (
        <Text key={i}>{r}</Text>
      ))}
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-error-subclasses/`
- [ ] Uses `TypeError`, `RangeError` constructors
- [ ] Uses `instanceof` with error subclasses
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for error subclasses
- [ ] Parity harness passes with 100% match in all 3 environments
