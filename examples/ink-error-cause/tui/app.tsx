import React from 'react';
import { Box, Text } from 'ink';

// Error with cause demo
function outer(): number {
  try {
    inner();
  } catch (e) {
    throw new Error('Outer error', { cause: e });
  }
  return 0;
}

function inner(): number {
  throw new Error('Inner error');
}

let outerError: Error | null = null;
try {
  outer();
} catch (e) {
  outerError = e as Error;
}

export default function App() {
  return (
    <Box flexDirection="column">
      <Text>Error.cause Demo</Text>
      <Text>Error message: {outerError?.message ?? 'none'}</Text>
      <Text>Has cause: {outerError?.cause ? 'yes' : 'no'}</Text>
      {outerError?.cause && (
        <Text>Cause: {(outerError.cause as Error).message}</Text>
      )}
    </Box>
  );
}
