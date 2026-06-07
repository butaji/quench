# Task 108: `ink-context-advanced` Example — Context DisplayName, DefaultValue, Multiple Providers

**Priority:** P1-High
**Phase:** 11 — React Pattern Coverage
**Depends on:** 078

## Problem

React Context with `displayName`, `defaultValue`, and nested/multiple providers is a common pattern. No existing Ink example exercises the full Context API surface.

## Ink Example

```tsx
// examples/ink-context-advanced/tui/app.tsx
import React, { createContext, useContext } from 'react';
import { Box, Text } from 'ink';

interface Theme {
  name: string;
  primary: string;
}

const defaultTheme: Theme = { name: 'light', primary: 'white' };
const ThemeContext = createContext<Theme>(defaultTheme);
ThemeContext.displayName = 'ThemeContext';

const LocaleContext = createContext('en');
LocaleContext.displayName = 'LocaleContext';

function ThemedText({ children }: { children: string }) {
  const theme = useContext(ThemeContext);
  const locale = useContext(LocaleContext);
  return (
    <Text color={theme.primary}>
      [{locale}] {children}
    </Text>
  );
}

export default function App() {
  return (
    <ThemeContext.Provider value={{ name: 'dark', primary: 'blue' }}>
      <LocaleContext.Provider value="fr">
        <Box flexDirection="column">
          <ThemedText>Hello</ThemedText>
          <Text>Context names: {ThemeContext.displayName}, {LocaleContext.displayName}</Text>
        </Box>
      </LocaleContext.Provider>
    </ThemeContext.Provider>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-context-advanced/`
- [ ] Uses `createContext` with `defaultValue`
- [ ] Sets `displayName` on context
- [ ] Uses nested/multiple providers
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
