# Task 160: `ink-regexp-named-groups` Example — Named Capture Groups

**Priority:** P2-Medium
**Phase:** 14 — Runtime API Completion
**Depends on:** 099

## Problem

RegExp named capture groups (`(?<name>...)` ) are an ES2018 feature for extracting matched substrings by name. No existing Ink example exercises them.

## Ink Example

```tsx
// examples/ink-regexp-named-groups/tui/app.tsx
import React from 'react';
import { Box, Text } from 'ink';

const datePattern = /(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})/;
const match = datePattern.exec('2024-03-15');
const groups = match?.groups;

const emailPattern = /(?<user>[\w.]+)@(?<domain>\w+\.\w+)/;
const emailMatch = emailPattern.exec('alice@example.com');

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Year: {groups?.year}</Text>
      <Text>Month: {groups?.month}</Text>
      <Text>Day: {groups?.day}</Text>
      <Text>User: {emailMatch?.groups?.user}</Text>
      <Text>Domain: {emailMatch?.groups?.domain}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-regexp-named-groups/`
- [ ] Uses `(?<name>...)` named capture groups
- [ ] Uses `.groups` property on match result
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
