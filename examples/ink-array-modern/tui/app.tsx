// ink-array-modern example — demonstrates modern ES2022+ array methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Modern array methods are standard JavaScript runtime features.

import React from 'react';
import { Box, Text } from 'ink';

// --- flat and flatMap ---
const nested = [[1, 2], [3, [4, 5]]];
const flat1 = nested.flat();
const flat2 = nested.flat(2); // flatten depth 2

const words = ['hello', 'world'];
const flatMapped = words.flatMap(w => [w, w.length]);

// --- at (ES2022) ---
const nums = [10, 20, 30, 40, 50];

// --- toSorted, toReversed (ES2023) ---
const unsorted = [3, 1, 4, 1, 5, 9, 2, 6];

// --- includes (ES2016) ---
const hasTwenty = nums.includes(20);
const hasHundred = nums.includes(100);

// --- findLast, findLastIndex (ES2023) ---
const evens = [2, 4, 6, 8, 10];
const lastEven = evens.findLast(n => n < 8);
const lastEvenIdx = evens.findLastIndex(n => n < 8);

// --- find, findIndex (baseline) ---
const people = [
  { name: 'Alice', age: 30 },
  { name: 'Bob', age: 25 },
  { name: 'Charlie', age: 35 },
];
const foundPerson = people.find(p => p.age > 28);
const foundIdx = people.findIndex(p => p.age > 28);

// --- filter and map (baseline) ---
const filtered = nums.filter(n => n > 15);
const mapped = nums.map(n => n * 2);

// --- reduce (baseline) ---
const sum = nums.reduce((acc, n) => acc + n, 0);

// --- some, every (baseline) ---
const allPositive = nums.every(n => n > 0);
const hasLarge = nums.some(n => n > 35);

// --- copyWithin (baseline) ---
const arr1 = [1, 2, 3, 4, 5];
const copied = arr1.copyWithin(0, 3); // [4, 5, 3, 4, 5]

// --- entries, keys, values ---
const fruits = ['apple', 'banana', 'cherry'];

export default function ArrayModernDemo() {
  const results: string[] = [];

  // flat
  results.push(`flat(1): [${flat1.join(', ')}]`);
  results.push(`flat(2): [${flat2.join(', ')}]`);

  // flatMap
  results.push(`flatMap: [${flatMapped.join(', ')}]`);

  // at
  results.push(`at(0): ${nums.at(0)}`);
  results.push(`at(-1): ${nums.at(-1)}`);
  results.push(`at(-2): ${nums.at(-2)}`);

  // toSorted, toReversed (immutable)
  results.push(`toSorted: [${unsorted.toSorted().join(', ')}]`);
  results.push(`toReversed: [${unsorted.toReversed().join(', ')}]`);
  results.push(`original unchanged: [${unsorted.join(', ')}]`);

  // includes
  results.push(`includes(20): ${hasTwenty}`);
  results.push(`includes(100): ${hasHundred}`);

  // findLast, findLastIndex
  results.push(`findLast < 8: ${lastEven}`);
  results.push(`findLastIndex < 8: ${lastEvenIdx}`);

  // find, findIndex
  results.push(`find age > 28: ${foundPerson?.name}`);
  results.push(`findIndex age > 28: ${foundIdx}`);

  // filter, map
  results.push(`filter > 15: [${filtered.join(', ')}]`);
  results.push(`map * 2: [${mapped.join(', ')}]`);

  // reduce
  results.push(`reduce sum: ${sum}`);

  // some, every
  results.push(`every > 0: ${allPositive}`);
  results.push(`some > 35: ${hasLarge}`);

  // copyWithin
  results.push(`copyWithin(0, 3): [${copied.join(', ')}]`);

  // Iteration methods
  const entries = Array.from(fruits.entries()).map(([i, v]) => `${i}:${v}`).join(', ');
  const keys = Array.from(fruits.keys()).join(', ');
  const values = Array.from(fruits.values()).join(', ');
  results.push(`entries: ${entries}`);
  results.push(`keys: ${keys}`);
  results.push(`values: ${values}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Modern Array Methods Demo</Text>
      <Text dimColor>ES2016-ES2023 array features</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
