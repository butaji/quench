// ink-array-mutators example — demonstrates array mutator methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: These methods mutate the array in place and return the changed element.
// NOTE: sort() and reverse() have known rq codegen differences.

import React from 'react';
import { Box, Text } from 'ink';

// --- push, pop, shift, unshift ---
const arr1 = [1, 2, 3];
const pushed = arr1.push(4);
const arr2 = [1, 2, 3];
const popped = arr2.pop();
const arr3 = [1, 2, 3];
const shifted = arr3.shift();
const arr4 = [1, 2, 3];
const unshifted = arr4.unshift(0);

// --- splice ---
const arr5 = ['a', 'b', 'c', 'd'];
const spliced1 = arr5.splice(1, 2); // remove 2 elements starting at index 1
const arr6 = ['a', 'b', 'c', 'd'];
const spliced2 = arr6.splice(1, 0, 'x', 'y'); // insert without removing
const arr7 = ['a', 'b', 'c', 'd'];
const spliced3 = arr7.splice(2, 1, 'z'); // replace one element

export default function ArrayMutatorsDemo() {
  const results: string[] = [];

  // push
  results.push(`push(4) returns: ${pushed}`);
  results.push(`arr after push: [${arr1.join(', ')}]`);

  // pop
  results.push(`pop() returns: ${popped}`);
  results.push(`arr after pop: [${arr2.join(', ')}]`);

  // shift
  results.push(`shift() returns: ${shifted}`);
  results.push(`arr after shift: [${arr3.join(', ')}]`);

  // unshift
  results.push(`unshift(0) returns: ${unshifted}`);
  results.push(`arr after unshift: [${arr4.join(', ')}]`);

  results.push('');

  // splice remove
  results.push(`splice(1, 2) returns: [${spliced1.join(', ')}]`);
  results.push(`arr after splice: [${arr5.join(', ')}]`);

  results.push('');

  // splice insert
  results.push(`splice(1, 0, 'x', 'y') returns: [${spliced2.join(', ')}]`);
  results.push(`arr after splice: [${arr6.join(', ')}]`);

  results.push('');

  // splice replace
  results.push(`splice(2, 1, 'z') returns: [${spliced3.join(', ')}]`);
  results.push(`arr after splice: [${arr7.join(', ')}]`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Array Mutators Demo</Text>
      <Text dimColor>Methods that mutate the array in place</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
