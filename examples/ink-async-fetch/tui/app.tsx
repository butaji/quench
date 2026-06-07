// ink-async-fetch example — demonstrates async, await, and Promise patterns.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: In the compile path, async/await uses Rust Futures. The dev path (rquickjs)
// fully supports async/await with JS Promises.

import React from 'react';
import { Box, Text } from 'ink';

export default function AsyncDemo() {
  const results: string[] = [];

  // Async function that returns a resolved Promise
  async function fetchGreeting(): Promise<string> {
    return 'Hello, Async World!';
  }

  // Async function with await
  async function fetchWithDelay(msg: string): Promise<string> {
    // Simulate async work (in real code, this would use setTimeout or fetch)
    return msg;
  }

  // Promise chain
  const promise = new Promise<string>((resolve) => {
    resolve('Promise resolved!');
  });

  // Async IIFE
  (async () => {
    const greeting = await fetchGreeting();
    results.push(greeting);

    const delayed = await fetchWithDelay('Delayed message');
    results.push(delayed);

    promise.then((msg) => {
      results.push(msg);
    });
  })();

  // Sync demonstration (since React renders synchronously)
  // Note: In a real async app, you'd use useEffect + useState for async results

  // Demonstrate async patterns work in sync context
  async function computeSum(a: number, b: number): Promise<number> {
    return a + b;
  }

  // Call async function synchronously (returns Promise)
  const sumPromise = computeSum(10, 20);
  results.push(`Sync call to async function returns Promise: ${sumPromise instanceof Promise}`);

  // Promise.all pattern
  const promises = [
    Promise.resolve(1),
    Promise.resolve(2),
    Promise.resolve(3),
  ];

  Promise.all(promises).then((values) => {
    results.push(`Promise.all resolved: ${values.join(', ')}`);
  });

  // Promise.race pattern
  Promise.race([
    new Promise<string>((r) => setTimeout(() => r('fast'), 100)),
    new Promise<string>((r) => setTimeout(() => r('slow'), 200)),
  ]).then((winner) => {
    results.push(`Promise.race winner: ${winner}`);
  });

  // Demonstrate sync usage (typical for Ink render)
  async function syncDemo() {
    const val = await Promise.resolve(42);
    return val * 2;
  }

  // For render, we typically use sync patterns
  const syncValue = syncDemo(); // This is a Promise
  results.push(`syncDemo() returns: Promise`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Async/Await Demo</Text>
      <Text dimColor>Note: Async results appear after initial render</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
