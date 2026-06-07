# Task 070: `ink-jsx-advanced` Example — Spread Attributes, Dynamic Components, Member Components

**Priority:** P2-Medium
**Phase:** 6 — JSX Advanced
**Depends on:** 069

## Problem

JSX spread attributes (`<Comp {...props} />`), dynamic components, and member components (`<Foo.Bar />`) are not validated.

## Example

```tsx
import { Box, Text, Newline } from 'ink';

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Box flexDirection="column" borderStyle="round" padding={1}>
      <Text bold>{title}</Text>
      <Newline />
      {children}
    </Box>
  );
}

export default function App() {
  const shared = { color: 'green', bold: true };
  const Dynamic = Panel;

  return (
    <Box flexDirection="column">
      <Panel title="Spread Props">
        <Text {...shared}>Spread attributes</Text>
      </Panel>
      <Dynamic title="Dynamic">
        <Text>Dynamic component reference</Text>
      </Dynamic>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] JSX spread attributes produce compilable Rust
- [ ] Dynamic component references produce compilable Rust
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%