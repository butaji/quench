// Type annotation in catch clause example — demonstrates catch (err: Error | unknown).
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Helper that might throw
function risky(): string {
  throw new Error('Something went wrong');
}

// Helper with catch type annotation (Error | unknown)
function safeRisky(): string {
  try {
    return risky();
  } catch (err: Error | unknown) {
    if (err instanceof Error) {
      return err.message;
    } else {
      return 'Unknown error';
    }
  }
}

// Helper with catch type annotation (any)
function anyCatch(): string {
  try {
    throw 'string error';
  } catch (err: any) {
    return `Caught: ${String(err)}`;
  }
}

export default function App() {
  const results: string[] = [];

  results.push(safeRisky());
  results.push(anyCatch());

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Catch Type Annotation Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
