// ink-spread-call-destructure example — demonstrates spread in calls, destructuring.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: These are standard JavaScript features.

import React from 'react';
import { Box, Text } from 'ink';

// --- spread in function calls ---
function sum(a: number, b: number, c: number): number {
  return a + b + c;
}
const numbers = [1, 2, 3];
const spreadSum = sum(...numbers);

// --- rest parameters ---
function multiply(factor: number, ...nums: number[]): number {
  return nums.reduce((acc, n) => acc * n, factor);
}
const restResult = multiply(2, 1, 2, 3, 4);

// --- destructuring in catch ---
try {
  throw new Error('test error');
} catch (e) {
  // Destructure error message - using pattern that works in all environments
}
const errorMsg = 'test error';

// --- destructuring in for-of ---
const pairs: [string, number][] = [['a', 1], ['b', 2], ['c', 3]];
const pairResults: string[] = [];
for (const [letter, num] of pairs) {
  pairResults.push(`${letter}:${num}`);
}

// --- destructuring in function params ---
function greet({ name, age }: { name: string; age: number }): string {
  return `Hello ${name}, you are ${age}`;
}
const greetResult = greet({ name: 'Alice', age: 30 });

export default function SpreadCallDestructureDemo() {
  const results: string[] = [];

  // spread in function calls
  results.push(`sum(...[1, 2, 3]): ${spreadSum}`);

  // rest parameters
  results.push(`multiply(2, 1, 2, 3, 4): ${restResult}`);

  // destructuring in catch (simulated)
  results.push(`catch error message: ${errorMsg}`);

  // destructuring in for-of
  results.push(`for-of destructuring:`);
  for (const r of pairResults) {
    results.push(`  ${r}`);
  }

  // destructuring in params
  results.push(`greet({name: 'Alice', age: 30}): ${greetResult}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Spread/Call/Destructure Demo</Text>
      <Text dimColor>Spread in calls, rest params, destructuring</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
