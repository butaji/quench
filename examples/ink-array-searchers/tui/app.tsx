// ink-array-searchers example — demonstrates array searching and filtering methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: These methods do not mutate the array (except sort which creates a new array).

import React from 'react';
import { Box, Text } from 'ink';

// --- indexOf, lastIndexOf ---
const nums1 = [1, 2, 3, 2, 4];
const idx1 = nums1.indexOf(2);
const lastIdx1 = nums1.lastIndexOf(2);
const idx2 = nums1.indexOf(5); // not found

// --- every, some ---
const nums2 = [2, 4, 6, 8, 10];
const allEven = nums2.every(n => n % 2 === 0);
const someOdd = nums2.some(n => n % 2 === 1);
const nums3 = [1, 2, 3, 4, 5];
const hasEven = nums3.some(n => n % 2 === 0);

// --- filter ---
const nums4 = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const evens = nums4.filter(n => n % 2 === 0);
const greaterThan5 = nums4.filter(n => n > 5);

// --- find, findIndex ---
const people = [
  { name: 'Alice', age: 30 },
  { name: 'Bob', age: 25 },
  { name: 'Charlie', age: 35 },
];
const foundPerson = people.find(p => p.age > 28);
const foundIdx = people.findIndex(p => p.age > 28);
const notFound = people.find(p => p.age > 100);
const notFoundIdx = people.findIndex(p => p.age > 100);

// --- includes (ES2016) ---
const nums5 = [1, 2, 3, 4, 5];
const has3 = nums5.includes(3);
const has99 = nums5.includes(99);

export default function ArraySearchersDemo() {
  const results: string[] = [];

  // indexOf, lastIndexOf
  results.push(`arr: [1, 2, 3, 2, 4]`);
  results.push(`indexOf(2): ${idx1}`);
  results.push(`lastIndexOf(2): ${lastIdx1}`);
  results.push(`indexOf(5): ${idx2}`);

  results.push('');

  // every, some
  results.push(`arr: [2, 4, 6, 8, 10]`);
  results.push(`every even: ${allEven}`);
  results.push(`some odd: ${someOdd}`);

  results.push(`arr: [1, 2, 3, 4, 5]`);
  results.push(`some even: ${hasEven}`);

  results.push('');

  // filter
  results.push(`arr: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]`);
  results.push(`filter even: [${evens.join(', ')}]`);
  results.push(`filter > 5: [${greaterThan5.join(', ')}]`);

  results.push('');

  // find, findIndex
  results.push(`find age > 28: ${foundPerson?.name ?? 'undefined'}`);
  results.push(`findIndex age > 28: ${foundIdx}`);
  results.push(`find age > 100: ${notFound?.name ?? 'undefined'}`);
  results.push(`findIndex age > 100: ${notFoundIdx}`);

  results.push('');

  // includes
  results.push(`arr: [1, 2, 3, 4, 5]`);
  results.push(`includes(3): ${has3}`);
  results.push(`includes(99): ${has99}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Array Searchers Demo</Text>
      <Text dimColor>Methods for searching and filtering arrays</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
