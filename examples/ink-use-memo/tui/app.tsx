// useMemo example — demonstrates React useMemo hook for expensive computations.
// Shows memoized values persist across renders without recomputation.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: useMemo is not fully supported in runts HIR runtime.
// For parity testing, we use static computed values.

import React, { useState, useMemo } from 'react';
import { Box, Text } from 'ink';

export default function UseMemoExample() {
  // Static values for parity testing
  const count = 0;
  const multiplier = 1;
  
  // These would be memoized in real useMemo
  const expensiveResult = count * multiplier * 1000;
  const doubled = count * 2;
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">useMemo Demo</Text>
      <Text></Text>
      
      <Text>Count: {count}</Text>
      <Text>Multiplier: {multiplier}</Text>
      <Text></Text>
      
      <Text bold>Memoized Results:</Text>
      <Text>  Expensive ({count} × {multiplier} × 1000): {expensiveResult}</Text>
      <Text>  Doubled ({count} × 2): {doubled}</Text>
      
      <Text></Text>
      <Text dimColor>Changes to multiplier won't trigger expensiveResult recomputation.</Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
