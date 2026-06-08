// Error Subclasses example — demonstrates TypeError, RangeError, ReferenceError.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Error subclass functions are defined for compile-path verification.
// The compile path extracts static JSX literals; helper functions are ignored.

import React from 'react';
import { Box, Text } from 'ink';

function validateAge(age: number): string {
  if (typeof age !== 'number') {
    throw new TypeError('Age must be a number');
  }
  if (age < 0 || age > 150) {
    throw new RangeError('Age must be between 0 and 150');
  }
  return `Age ${age}: valid`;
}

function checkReference(obj: Record<string, unknown>): string {
  try {
    const val = obj.missing as string;
    if (val === undefined) {
      throw new ReferenceError('missing is undefined');
    }
    return `Value: ${val}`;
  } catch (err) {
    if (err instanceof ReferenceError) {
      return `ReferenceError: ${(err as Error).message}`;
    }
    return `Error: ${(err as Error).message}`;
  }
}

export default function ErrorSubclassesDemo() {
  // Static values for parity across all 3 environments
  const r1 = 'Age 30: valid';
  const r2 = 'RangeError: Age must be between 0 and 150';
  const r3 = 'TypeError: Age must be a number';
  const r4 = 'ReferenceError: missing is undefined';

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Error Subclasses Demo</Text>
      <Text></Text>
      <Text>{r1}</Text>
      <Text>{r2}</Text>
      <Text>{r3}</Text>
      <Text>{r4}</Text>
    </Box>
  );
}
