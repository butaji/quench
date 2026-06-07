// ink-typeof-guard example — demonstrates typeof, instanceof, delete, and void.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function TypeofGuardDemo() {
  const results: string[] = [];

  // typeof with literals
  results.push(`typeof 'hello': ${typeof 'hello'}`);
  results.push(`typeof 42: ${typeof 42}`);
  results.push(`typeof true: ${typeof true}`);
  results.push(`typeof undefined: ${typeof undefined}`);
  results.push(`typeof null: ${typeof null}`);
  results.push(`typeof {}: ${typeof {}}`);
  results.push(`typeof []: ${typeof []}`);
  results.push(`typeof function: ${typeof function() {}}`);

  // instanceof
  const date = new Date('2024-01-15');
  const array: unknown = [1, 2, 3];
  const regexp = /test/;
  const map = new Map();
  results.push(`date instanceof Date: ${date instanceof Date}`);
  results.push(`array instanceof Array: ${array instanceof Array}`);
  results.push(`regexp instanceof RegExp: ${regexp instanceof RegExp}`);
  results.push(`map instanceof Map: ${map instanceof Map}`);

  // typeof guards
  function processValue(value: unknown): string {
    if (typeof value === 'string') {
      return `String: ${value.toUpperCase()}`;
    }
    if (typeof value === 'number') {
      return `Number: ${value * 2}`;
    }
    if (typeof value === 'boolean') {
      return `Boolean: ${value ? 'YES' : 'NO'}`;
    }
    if (typeof value === 'object') {
      if (value === null) return 'Null object';
      if (Array.isArray(value)) return `Array with ${value.length} items`;
      return 'Object';
    }
    return 'Unknown';
  }

  results.push(processValue('hello'));
  results.push(processValue(42));
  results.push(processValue(true));
  results.push(processValue(null));
  results.push(processValue([1, 2, 3]));
  results.push(processValue({ a: 1 }));

  // typeof with function parameters
  function add(a: unknown, b: unknown): string {
    if (typeof a === 'number' && typeof b === 'number') {
      return `${a} + ${b} = ${a + b}`;
    }
    return 'Cannot add non-numbers';
  }
  results.push(add(5, 3));
  results.push(add('hello', 'world'));

  // typeof for type narrowing
  const values: (string | number | boolean)[] = ['hello', 42, true, 'world', 100];
  const stringVals: string[] = [];
  const numberVals: number[] = [];
  for (const v of values) {
    if (typeof v === 'string') {
      stringVals.push(v);
    } else if (typeof v === 'number') {
      numberVals.push(v);
    }
  }
  results.push(`strings: ${stringVals.join(', ')}`);
  results.push(`numbers: ${numberVals.join(', ')}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Typeof Guard Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
