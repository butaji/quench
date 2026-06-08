// ink-symbol-iterator example — demonstrates Symbol.iterator and custom iterables.
//
// Symbol.iterator allows objects to define custom iteration behavior.
// This is the protocol that for...of loops and spread operator use.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// --- Custom Range iterator ---
class Range {
  constructor(private start: number, private end: number) {}

  *[Symbol.iterator]() {
    for (let i = this.start; i <= this.end; i++) {
      yield i;
    }
  }
}

// --- Custom Map-like iterable ---
class PairList {
  private pairs: [string, number][] = [];

  add(key: string, value: number) {
    this.pairs.push([key, value]);
  }

  *[Symbol.iterator]() {
    for (const pair of this.pairs) {
      yield pair;
    }
  }
}

// --- Object with Symbol.iterator ---
const numberedItems = {
  items: ['Apple', 'Banana', 'Cherry'],
  *[Symbol.iterator]() {
    for (let i = 0; i < this.items.length; i++) {
      yield `${i + 1}. ${this.items[i]}`;
    }
  }
};

// --- String is already iterable ---
function iterateString(s: string): string[] {
  const result: string[] = [];
  for (const char of s) {
    result.push(char);
  }
  return result;
}

// --- Array is already iterable ---
function iterateArray<T>(arr: T[]): string[] {
  const result: string[] = [];
  for (const item of arr) {
    result.push(String(item));
  }
  return result;
}

// --- Use iterator protocol directly ---
function useIteratorManual() {
  const range = new Range(1, 5);
  const iterator = range[Symbol.iterator]();
  const result: number[] = [];
  let next = iterator.next();
  while (!next.done) {
    result.push(next.value);
    next = iterator.next();
  }
  return result;
}

export default function App() {
  // Create iterables
  const range = new Range(1, 5);
  const chars = iterateString('hello');
  const nums = iterateArray([10, 20, 30]);

  // Custom iterable
  const pairList = new PairList();
  pairList.add('x', 1);
  pairList.add('y', 2);
  pairList.add('z', 3);

  // Use for...of with different iterables
  const rangeNums: number[] = [];
  for (const n of range) {
    rangeNums.push(n);
  }

  const objItems: string[] = [];
  for (const item of numberedItems) {
    objItems.push(item);
  }

  const manualNums = useIteratorManual();

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Symbol.iterator & Custom Iterables Demo</Text>
      <Text dimColor>Defining custom iteration behavior</Text>
      <Text></Text>

      <Text>Custom Range class with [Symbol.iterator]:</Text>
      <Text>  Range(1,5) via for...of: {rangeNums.join(', ')}</Text>
      <Text>  Range(1,5) via iterator protocol: {manualNums.join(', ')}</Text>

      <Text></Text>
      <Text>String is built-in iterable:</Text>
      <Text>  iterateString("hello"): {chars.join(', ')}</Text>

      <Text></Text>
      <Text>Array is built-in iterable:</Text>
      <Text>  iterateArray([10,20,30]): {nums.join(', ')}</Text>

      <Text></Text>
      <Text>Custom PairList with Symbol.iterator:</Text>
      <Text>  pairs: {Array.from(pairList).map(p => `${p[0]}=${p[1]}`).join(', ')}</Text>

      <Text></Text>
      <Text>Object with inline [Symbol.iterator]:</Text>
      <Text>  numbered: {objItems.join(', ')}</Text>

      <Text></Text>
      <Text>Spread with iterables:</Text>
      <Text>  [...new Range(10, 12)]: {[...new Range(10, 12)].join(', ')}</Text>
    </Box>
  );
}
