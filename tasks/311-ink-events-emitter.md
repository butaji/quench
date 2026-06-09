# Task 311: `ink-events-emitter` Example — `EventEmitter`

**Priority:** P2-Medium
**Phase:** 25 — Node.js Runtime APIs
**Depends on:** 310

## Problem

`EventEmitter` is the Node.js events module for pub/sub patterns. No existing Ink example exercises it.

## Ink Example

```tsx
// examples/ink-events-emitter/tui/app.tsx
import React, { useState, useEffect } from 'react';
import { Box, Text } from 'ink';

class SimpleEmitter {
  private listeners: Map<string, Array<(...args: any[]) => void>> = new Map();

  on(event: string, cb: (...args: any[]) => void): void {
    if (!this.listeners.has(event)) this.listeners.set(event, []);
    this.listeners.get(event)!.push(cb);
  }

  emit(event: string, ...args: any[]): void {
    this.listeners.get(event)?.forEach(cb => cb(...args));
  }
}

export default function App() {
  const [message, setMessage] = useState('');

  useEffect(() => {
    const emitter = new SimpleEmitter();
    emitter.on('greet', (name: string) => setMessage(`Hello, ${name}`));
    emitter.emit('greet', 'World');
  }, []);

  return (
    <Box flexDirection="column">
      <Text>Message: {message}</Text>
    </Box>
  );
}
```


## HIR Coverage

- Standard `Expr`/`Stmt` variants

## Compile-Path Codegen

- Standard `quote_codegen` expression + statement codegen

## Acceptance Criteria

- [ ] Example exists at `examples/ink-events-emitter/`
- [ ] Uses custom EventEmitter pattern (or `events` module)
- [ ] Renders identically in deno and `runts dev` (100% output match)
- [ ] Compile path generates compilable Rust
- [ ] Parity harness passes with 100% match in all 3 environments
