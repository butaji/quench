# Task 338: `ink-compound-components` Example — Compound Component Pattern

**Priority:** P2-Medium
**Phase:** 27 — React Patterns
**Depends on:** 337

## Problem

Compound components (`<Tabs><Tabs.List><Tabs.Panel>...</Tabs.Panel></Tabs.List></Tabs>`) share implicit state via Context. No existing Ink example exercises this pattern.

## Ink Example

```tsx
// examples/ink-compound-components/tui/app.tsx
import React, { createContext, useContext, useState } from 'react';
import { Box, Text } from 'ink';

const TabsContext = createContext<{ active: string; setActive: (id: string) => void } | null>(null);

function Tabs({ children }: { children: React.ReactNode }) {
  const [active, setActive] = useState('a');
  return (
    <TabsContext.Provider value={{ active, setActive }}>
      <Box flexDirection="column">{children}</Box>
    </TabsContext.Provider>
  );
}

function List({ children }: { children: React.ReactNode }) {
  return <Box flexDirection="row" gap={2}>{children}</Box>;
}

function Trigger({ id, children }: { id: string; children: React.ReactNode }) {
  const ctx = useContext(TabsContext);
  const active = ctx?.active === id;
  return <Text bold={active}>{children}</Text>;
}

function Panel({ id, children }: { id: string; children: React.ReactNode }) {
  const ctx = useContext(TabsContext);
  if (ctx?.active !== id) return null;
  return <Box>{children}</Box>;
}

Tabs.List = List;
Tabs.Trigger = Trigger;
Tabs.Panel = Panel;

export default function App() {
  return (
    <Tabs>
      <Tabs.List>
        <Tabs.Trigger id="a">Tab A</Tabs.Trigger>
        <Tabs.Trigger id="b">Tab B</Tabs.Trigger>
      </Tabs.List>
      <Tabs.Panel id="a"><Text>Panel A</Text></Tabs.Panel>
      <Tabs.Panel id="b"><Text>Panel B</Text></Tabs.Panel>
    </Tabs>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-compound-components/`
- [ ] Uses compound component pattern with Context
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
