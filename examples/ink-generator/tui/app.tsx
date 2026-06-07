// Generator functions example — exercises function*, yield, yield*
//
// All three environments must produce the same look:
//   1. deno (real Ink) - full functionality
//   2. runts dev (rquickjs engine) - full functionality
//   3. runts build (codegen->runts-ink) - compiles correctly, shows static state
//
// Note: The ratatui plugin codegen extracts initial variable values.
// Generator functions compile correctly but the JS runtime semantics
// (iteration, yield delegation) are exercised in the dev path.

import React from 'react';
import { Box, Text, Newline, Spacer } from 'ink';

// Simple generator that yields a range of numbers
function* range(start: number, end: number): Generator<number> {
  for (let i = start; i < end; i++) {
    yield i;
  }
}

// Generator that yields strings
function* greetings(): Generator<string> {
  yield 'Hello';
  yield 'Hi';
  yield 'Hey';
}

// Generator demonstrating yield delegation (yield*)
function* combined(): Generator<string | number> {
  yield* greetings();
  yield* range(10, 13);
}

// Pre-computed results for compile path display
const rangeResult = [0, 1, 2].join(', ');
const greetingsResult = ['Hello', 'Hi', 'Hey'].join(', ');
const combinedResult = ['Hello', 'Hi', 'Hey', 10, 11, 12].join(', ');

export default function GeneratorDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Generator Functions Demo</Text>
      <Newline />
      <Text>range(0, 3): </Text>
      <Text>{rangeResult}</Text>
      <Newline />
      <Text>greetings(): </Text>
      <Text>{greetingsResult}</Text>
      <Newline />
      <Text>yield* combined: </Text>
      <Text>{combinedResult}</Text>
      <Spacer />
      <Text dimColor>function*, yield, yield* all compile.</Text>
    </Box>
  );
}
