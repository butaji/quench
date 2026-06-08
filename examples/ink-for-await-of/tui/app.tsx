// ink-for-await-of example — demonstrates for await...of async iteration
//
// This example exercises the for-await-of statement for iterating
// over async iterables, including:
// - for await...of with async generators
// - for await...of with Promise arrays
// - Sequential vs parallel processing
//
// Note: Since this is a TUI context, we use synchronous patterns
// to demonstrate the syntax and show the compiled results.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Async function that returns promises
async function fetchUser(id: number): Promise<{ id: number; name: string }> {
  const names = ['Alice', 'Bob', 'Charlie', 'Diana', 'Eve'];
  return { id, name: names[id % names.length] };
}

// Async generator function
async function* asyncGenerator(max: number): AsyncGenerator<number> {
  for (let i = 1; i <= max; i++) {
    // Simulate async delay
    await new Promise(resolve => setTimeout(resolve, 1));
    yield i;
  }
}

// Async iterable class
class AsyncIterableNumbers {
  private items: number[];

  constructor(items: number[]) {
    this.items = items;
  }

  async *[Symbol.asyncIterator](): AsyncIterator<number> {
    for (const item of this.items) {
      await new Promise(resolve => setTimeout(resolve, 1));
      yield item;
    }
  }
}

// For await with sequential processing
async function processSequential(ids: number[]): Promise<string[]> {
  const names: string[] = [];
  for await (const id of ids) {
    const user = await fetchUser(id);
    names.push(user.name);
  }
  return names;
}

// For await with async generator
async function collectGeneratorOutput(max: number): Promise<number[]> {
  const results: number[] = [];
  for await (const num of asyncGenerator(max)) {
    results.push(num);
  }
  return results;
}

// For await with async iterable class
async function collectIterableOutput(items: number[]): Promise<number[]> {
  const results: number[] = [];
  for await (const num of new AsyncIterableNumbers(items)) {
    results.push(num);
  }
  return results;
}

// Run all async operations and collect results synchronously
async function runAll(): Promise<{
  sequential: string[];
  generator: number[];
  iterable: number[];
}> {
  const [sequential, generator, iterable] = await Promise.all([
    processSequential([1, 2, 3]),
    collectGeneratorOutput(3),
    collectIterableOutput([10, 20, 30])
  ]);

  return { sequential, generator, iterable };
}

// Pre-compute values for display
const userIds = [1, 2, 3];
const generatorMax = 3;
const iterableItems = [10, 20, 30];

// Note: In a real async scenario, these would be awaited.
// Here we demonstrate the syntax pattern.
const sequentialPattern = 'for await (const id of ids) { ... }';
const generatorPattern = 'for await (const num of asyncGen(3)) { ... }';
const iterablePattern = 'for await (const num of new AsyncIterable([...])) { ... }';

export default function ForAwaitDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">for await...of Async Iteration</Text>
      <Text></Text>
      <Text>Syntax patterns:</Text>
      <Text>  Sequential: {sequentialPattern}</Text>
      <Text>  Generator: {generatorPattern}</Text>
      <Text>  Iterable: {iterablePattern}</Text>
      <Text></Text>
      <Text>Processing patterns:</Text>
      <Text>  userIds: [{userIds.join(', ')}]</Text>
      <Text>  generator max: {generatorMax}</Text>
      <Text>  iterable items: [{iterableItems.join(', ')}]</Text>
      <Text></Text>
      <Text color="yellow">Note: Async iteration requires Promise.all or</Text>
      <Text color="yellow">useEffect with state for actual results.</Text>
    </Box>
  );
}
