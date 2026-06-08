# Task 270: `ink-decorator-metadata` Example — `emitDecoratorMetadata`

**Priority:** P3-Low
**Phase:** 22 — TypeScript Configuration Edge Cases
**Depends on:** 269

## Problem

`emitDecoratorMetadata` causes TypeScript to emit type metadata for decorated declarations, enabling runtime reflection. Task 074 covers decorators; no example covers metadata emission.

## Ink Example

```tsx
// tsconfig.json with emitDecoratorMetadata: true, experimentalDecorators: true
// examples/ink-decorator-metadata/tui/app.tsx
import React from 'react';
import 'reflect-metadata';
import { Box, Text } from 'ink';

function LogType(target: any, propertyKey: string) {
  const type = Reflect.getMetadata('design:type', target, propertyKey);
  // eslint-disable-next-line no-console
  console.log(`${propertyKey} type: ${type?.name}`);
}

class Example {
  @LogType
  value: string = 'test';
}

const ex = new Example();

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Value: {ex.value}</Text>
    </Box>
  );
}
```

## Acceptance Criteria

- [ ] Example exists at `examples/ink-decorator-metadata/`
- [ ] Includes `tsconfig.json` with `emitDecoratorMetadata: true`
- [ ] Uses decorator that reads `Reflect` metadata
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path respects `emitDecoratorMetadata`
- [ ] Parity harness passes with 100% match in all 3 environments
