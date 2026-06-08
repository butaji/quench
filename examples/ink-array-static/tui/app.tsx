// ink-array-static example — demonstrates Array static methods
//
// This example exercises the static Array methods:
// - Array.from(arrayLike, mapFn?) - creates array from array-like or iterable
// - Array.of(...items) - creates array from arguments
// - Array.isArray(value) - checks if value is an array
// - Array.from with mapping function
// - Array.from with async iterables
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Array.from with array-like object
function arrayLikeToArray(arr: { length: number; [index: number]: unknown }): string[] {
  return Array.from(arr) as string[];
}

const arrayLike = { 0: 'a', 1: 'b', 2: 'c', length: 3 };
const fromArrayLike = Array.from(arrayLike);

// Array.from with mapping function
const numbers = [1, 2, 3, 4, 5];
const doubled = Array.from(numbers, x => x * 2);

// Array.from with generator
function* generateNumbers(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}
const fromGenerator = Array.from(generateNumbers());

// Array.of - create array from arguments
const ofExample1 = Array.of(1, 2, 3);
const ofExample2 = Array.of('a', 'b', 'c');
const ofExample3 = Array.of(); // empty array

// Array.isArray checks
const isArrayTests: [unknown, boolean][] = [
  [[], true],
  [[1, 2, 3], true],
  [new Array(5), true],
  ['hello', false],
  [123, false],
  [{ length: 3, 0: 'a', 1: 'b', 2: 'c' }, false], // array-like, not array
  [null, false],
  [undefined, false],
];

// Array.from with string (iterable)
const fromString = Array.from('hello');

// Array.from with Set
const set = new Set([1, 2, 3, 2, 1]);
const fromSet = Array.from(set);

// Array.from with Map entries
const map = new Map([['a', 1], ['b', 2]]);
const fromMapEntries = Array.from(map.entries());

// Array.of with mixed types
const mixedArray = Array.of(1, 'two', true, null, undefined);

// Range creation helper
function createRange(start: number, end: number): number[] {
  return Array.from({ length: end - start }, (_, i) => start + i);
}

const range = createRange(5, 10);

// Format array for display
function formatArray(arr: unknown[]): string {
  return `[${arr.join(', ')}]`;
}

export default function ArrayStaticDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Array.from / Array.of / Array.isArray</Text>
      <Text></Text>
      <Text>Array.from with array-like:</Text>
      <Text>  fromArrayLike: {formatArray(fromArrayLike)}</Text>
      <Text></Text>
      <Text>Array.from with map function:</Text>
      <Text>  numbers: {formatArray(numbers)}</Text>
      <Text>  doubled: {formatArray(doubled)}</Text>
      <Text></Text>
      <Text>Array.from with generator:</Text>
      <Text>  fromGenerator: {formatArray(fromGenerator)}</Text>
      <Text></Text>
      <Text>Array.of examples:</Text>
      <Text>  of(1,2,3): {formatArray(ofExample1)}</Text>
      <Text>  of(a,b,c): {formatArray(ofExample2)}</Text>
      <Text>  of(): {formatArray(ofExample3)}</Text>
      <Text>  of(mixed): {formatArray(mixedArray)}</Text>
      <Text></Text>
      <Text>Array.isArray tests:</Text>
      {isArrayTests.map(([val, expected]) => (
        <Text key={String(val)}>  {JSON.stringify(val)}: {expected ? 'true' : 'false'}</Text>
      ))}
      <Text></Text>
      <Text>Array.from with string:</Text>
      <Text>  from(hello): {formatArray(fromString)}</Text>
      <Text></Text>
      <Text>Array.from with Set:</Text>
      <Text>  fromSet: {formatArray(fromSet)}</Text>
      <Text></Text>
      <Text>Array.from with Map entries:</Text>
      <Text>  fromMapEntries: {formatArray(fromMapEntries)}</Text>
      <Text></Text>
      <Text>createRange(5, 10):</Text>
      <Text>  range: {formatArray(range)}</Text>
    </Box>
  );
}
