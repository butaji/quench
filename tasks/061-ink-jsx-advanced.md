# Task 061: `ink-jsx-advanced` Example — Spread Attributes, Dynamic Components, Fragments, Conditional Rendering

**Priority:** P2-Medium  
**Phase:** 6 — JSX Advanced  
**Depends on:** 060

## Problem

JSX spread attributes, dynamic components, fragments, and conditional rendering are not validated by a dedicated example.

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

export default function App({ showHeader, items }: { showHeader: boolean; items: string[] }) {
  const shared = { color: 'green', bold: true };
  const Dynamic = Panel;

  return (
    <Box flexDirection="column">
      {showHeader && <Text bold>Header</Text>}
      <Panel title="Spread Props">
        <Text {...shared}>Spread attributes</Text>
      </Panel>
      <Dynamic title="Dynamic">
        <Text>Dynamic component reference</Text>
      </Dynamic>
      <>
        {items.map((item, i) => (
          <Text key={item}>{i + 1}. {item}</Text>
        ))}
      </>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] JSX spread attributes produce compilable Rust
- [ ] Dynamic component references produce compilable Rust
- [ ] Fragments produce compilable Rust
- [ ] Conditional rendering (`&&` and ternary) produces compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
