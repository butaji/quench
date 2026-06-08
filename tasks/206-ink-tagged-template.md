# Task 206: `ink-tagged-template` Example — Tagged Template Literals

**Priority:** P1-High
**Phase:** 19 — Advanced Expression Patterns
**Depends on:** 205

## Problem

Tagged template literals (`tag`hello ${name}``) allow a function to process a template literal's strings and values. No existing Ink example exercises tagged templates.

## Ink Example

```tsx
// examples/ink-tagged-template/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

function highlight(strings: TemplateStringsArray, ...values: unknown[]): string {
  return strings.reduce((acc, str, i) => {
    const val = i < values.length ? `[${values[i]}]` : '';
    return acc + str + val;
  }, '');
}

const name = 'World';
const count = 42;

export default function App() {
  const result = highlight`Hello ${name}, count is ${count}`;

  return (
    <Box flexDirection="column">
      <Text>{result}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-tagged-template/`
- [ ] Uses tagged template literal syntax
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust for tagged templates
- [ ] Parity harness passes with 100% match in all 3 environments
