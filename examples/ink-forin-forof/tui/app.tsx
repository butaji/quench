// For-In/For-Of example — demonstrates for-in, for-of, and iterators.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function ForInForOfDemo() {
  const results: string[] = [];

  // for-in with object
  const obj = { a: 1, b: 2, c: 3 };
  const objKeys: string[] = [];
  for (const key in obj) {
    objKeys.push(`${key}=${obj[key as keyof typeof obj]}`);
  }
  results.push(`for-in: ${objKeys.join(', ')}`);

  // for-of with array
  const arr = ['x', 'y', 'z'];
  const arrValues: string[] = [];
  for (const val of arr) {
    arrValues.push(val);
  }
  results.push(`for-of: ${arrValues.join('-')}`);

  // for-of with numbers
  let numSum = 0;
  for (const n of [10, 20, 30]) {
    numSum = numSum + n;
  }
  results.push(`for-of nums sum: ${numSum}`);

  // for-in with let binding
  let sum = 0;
  const counts = { x: 1, y: 2 };
  for (const k in counts) {
    sum = sum + counts[k as keyof typeof counts];
  }
  results.push(`for-in sum: ${sum}`);

  // for-of simple
  let count = 0;
  for (const _ of [1, 2, 3]) {
    count = count + 1;
  }
  results.push(`for-of count: ${count}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">For-In/For-Of Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
