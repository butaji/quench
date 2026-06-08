// ink-promise-resolve-reject example — demonstrates Promise.resolve and Promise.reject.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Promises are standard JavaScript runtime features.

import React from 'react';
import { Box, Text } from 'ink';

// Promise.resolve creates a resolved promise
const resolvedValue = Promise.resolve(42);
const resolvedObj = Promise.resolve({ x: 1, y: 2 });

// Promise.reject with catch to avoid unhandled rejection
const rejectedPromise = Promise.reject(new Error('test error')).catch(e => e.message);

// Chain then/catch
const chained = Promise.resolve(10)
  .then(x => x * 2)
  .then(x => x + 5)
  .then(x => String(x));

// Promise.all
const p1 = Promise.resolve(1);
const p2 = Promise.resolve(2);
const p3 = Promise.resolve(3);

// Promise.race
const fast = Promise.resolve('fast');
const slow = new Promise(r => setTimeout(() => r('slow'), 1000));

export default function PromiseResolveRejectDemo() {
  const results: string[] = [];

  // Promise.resolve creates a resolved promise
  results.push(`Promise.resolve(42): Promise<42>`);
  results.push(`Promise.resolve({x:1}): Promise<{x:1}>`);

  // Promise.reject with catch
  results.push(`Promise.reject(Error).catch: Promise<error message>`);

  // Chaining
  results.push(`Promise.resolve(10).then(x=>x*2).then(x=>x+5): Promise<25>`);

  // Promise.all
  results.push(`Promise.all([1,2,3]): Promise<[1,2,3]>`);

  // Promise.race
  results.push(`Promise.race([fast, slow]): Promise<fast>`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Promise.resolve/reject Demo</Text>
      <Text dimColor>Promise.resolve, Promise.reject, Promise.all, Promise.race</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
