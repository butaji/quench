// Throw expression example — error handling in expression position
//
// Exercises:
//   - throw inside IIFE (expression-position equivalent)
//   - nullish coalescing with fallback throw
//   - assertDefined helper pattern
//
// All three environments must produce the same look:
//   1. deno (real Ink) - full functionality
//   2. runts dev (rquickjs engine) - full functionality
//   3. runts build (codegen->runts-ink) - compiles correctly, shows static state

import React from 'react';
import { Box, Text } from 'ink';

function assertDefined<T>(value: T | undefined, msg: string): T {
  return value ?? (() => { throw new Error(msg); })();
}

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Throw Expression Demo</Text>
      <Text>Hello World</Text>
      <Text dimColor>assertDefined with throw IIFE works.</Text>
    </Box>
  );
}
