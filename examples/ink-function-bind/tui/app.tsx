import React from 'react';
import { Box, Text } from 'ink';

function greet(greeting: string, name: string): string {
  return `${greeting}, ${name}!`;
}

function sum(a: number, b: number, c: number): number {
  return a + b + c;
}

const greetAlice = greet.bind(null, 'Hello');
const partialSum = sum.bind(null, 10);

export default function App() {
  const callResult = greet.call(null, 'Hi', 'Bob');
  const applyResult = greet.apply(null, ['Hey', 'Alice']);

  return (
    <Box flexDirection="column">
      <Text>Function.bind, call, apply Demo</Text>
      <Text>Bind: {greetAlice('World')}</Text>
      <Text>Call: {callResult}</Text>
      <Text>Apply: {applyResult}</Text>
      <Text>Partial sum(10,2,3): {partialSum(2, 3)}</Text>
    </Box>
  );
}
