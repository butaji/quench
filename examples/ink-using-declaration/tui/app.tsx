// ink-using-declaration example — demonstrates `using` and `await using` (ES2024 / TS 5.2)
//
// `using` and `await using` enable explicit resource management with automatic cleanup
// via Symbol.dispose and Symbol.asyncDispose.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Resource with Symbol.dispose for explicit cleanup
function createResource(name: string): { name: string; [Symbol.dispose]: () => void } {
  return {
    name,
    [Symbol.dispose]() {
      // Cleanup happens automatically at end of scope
    },
  };
}

export default function UsingDemo() {
  const results: string[] = [];

  // Basic using declaration
  {
    using r1 = createResource('Resource-1');
    results.push(`using: ${r1.name}`);
  }
  // r1 is disposed here

  // Multiple using declarations in same scope
  {
    using r2 = createResource('First');
    using r3 = createResource('Second');
    results.push(`multiple: ${r2.name}, ${r3.name}`);
  }

  // Nested using scopes
  {
    using outer = createResource('Outer');
    results.push(`nested outer: ${outer.name}`);
    {
      using inner = createResource('Inner');
      results.push(`nested inner: ${inner.name}`);
    }
    // inner is disposed
    results.push(`after inner: ${outer.name}`);
  }
  // outer is disposed

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Using Declaration Demo</Text>
      <Text dimColor>ES2024 / TS 5.2</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
