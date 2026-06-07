// Try-Catch example — demonstrates try, catch, finally, throw.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Error handling in the compile path has limitations.

import React from 'react';
import { Box, Text } from 'ink';

// Helper that might throw
function riskyDivision(a: number, b: number): number {
  if (b === 0) {
    throw new Error('Division by zero');
  }
  return a / b;
}

// Helper with try-catch
function safeDivide(a: number, b: number): string {
  try {
    const result = riskyDivision(a, b);
    return `Result: ${result}`;
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    return `Error: ${msg}`;
  } finally {
    // finally always runs
  }
}

// Demonstrate nested try-catch
function nestedTryCatch(): string {
  try {
    try {
      throw new Error('Inner error');
    } catch (inner) {
      // Rethrow
      throw new Error('Outer error after catching inner');
    }
  } catch (outer) {
    const msg = outer instanceof Error ? outer.message : String(outer);
    return `Caught: ${msg}`;
  }
}

// Demonstrate multiple catch blocks (TypeScript feature)
function multiCatch(action: string): string {
  try {
    if (action === 'throw number') {
      throw 42;
    } else if (action === 'throw string') {
      throw 'error string';
    } else if (action === 'throw object') {
      throw { code: 'ERR', message: 'custom error' };
    } else {
      return 'No error';
    }
  } catch (e) {
    if (typeof e === 'number') {
      return `Caught number: ${e}`;
    } else if (typeof e === 'string') {
      return `Caught string: ${e}`;
    } else if (typeof e === 'object' && e !== null) {
      const obj = e as { code?: string; message?: string };
      return `Caught object: ${obj.code || 'unknown'} - ${obj.message || 'no message'}`;
    }
    return 'Unknown error type';
  }
}

export default function TryCatchDemo() {
  const results: string[] = [];

  // Test various try-catch scenarios
  results.push(safeDivide(10, 2));
  results.push(safeDivide(5, 0));
  results.push(nestedTryCatch());
  results.push(multiCatch('throw number'));
  results.push(multiCatch('throw string'));
  results.push(multiCatch('throw object'));

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Try-Catch Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
