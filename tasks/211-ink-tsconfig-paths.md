# Task 211: `ink-tsconfig-paths` Example — tsconfig `paths` Mapping

**Priority:** P1-High
**Phase:** 19 — TypeScript Configuration
**Depends on:** 210

## Problem

tsconfig `paths` mapping (`"@/*": ["./src/*"]`) is a common TypeScript feature for aliased imports. No existing Ink example exercises path mapping.

## Ink Example

```tsx
// examples/ink-tsconfig-paths/tsconfig.json
{
  "compilerOptions": {
    "paths": {
      "@/components/*": ["./components/*"],
      "@/utils/*": ["./utils/*"]
    }
  }
}

// examples/ink-tsconfig-paths/components/Header.tsx
import React from 'react';
import { Text } from 'ink';

export function Header({ title }: { title: string }) {
  return <Text bold>{title}</Text>;
}

// examples/ink-tsconfig-paths/utils/greet.ts
export function greet(name: string): string {
  return `Hello, ${name}`;
}

// examples/ink-tsconfig-paths/tui/app.tsx
import React from 'react';
import { Box } from 'ink';
import { Header } from '@/components/Header';
import { greet } from '@/utils/greet';

export default function App() {
  return (
    <Box flexDirection="column">
      <Header title={greet('World')} />
    </Box>
  );
}
```


## HIR Coverage

- Parser directives (no runtime HIR impact)

## Compile-Path Codegen

- Parser/bundler configuration (no runtime codegen)

## Acceptance Criteria

- [ ] Example exists at `examples/ink-tsconfig-paths/`
- [ ] Uses `tsconfig.json` with `paths` mapping
- [ ] Imports via path alias (`@/components/...`)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path resolves path aliases during codegen
- [ ] Parity harness passes with 100% match in all 3 environments
