// ink-function-params example — demonstrates default parameters and rest parameters.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function FunctionParamsDemo() {
  const results: string[] = [];

  // Default parameters
  function greet(name: string = 'Guest', greeting: string = 'Hello'): string {
    return `${greeting}, ${name}!`;
  }
  results.push(greet());
  results.push(greet('Alice'));
  results.push(greet('Bob', 'Hi'));
  results.push(greet(undefined, 'Hey'));

  // Rest parameters
  function sum(...nums: number[]): number {
    return nums.reduce((a, b) => a + b, 0);
  }
  results.push(`sum(): ${sum()}`);
  results.push(`sum(1): ${sum(1)}`);
  results.push(`sum(1, 2): ${sum(1, 2)}`);
  results.push(`sum(1, 2, 3, 4, 5): ${sum(1, 2, 3, 4, 5)}`);

  // Rest with other params
  function concat(sep: string, ...parts: string[]): string {
    return parts.join(sep);
  }
  results.push(`concat('-', 'a', 'b', 'c'): ${concat('-', 'a', 'b', 'c')}`);
  results.push(`concat(', ', 'x'): ${concat(', ', 'x')}`);

  // Default and rest combined
  function average(label: string = 'Average', ...nums: number[]): string {
    if (nums.length === 0) return `${label}: N/A`;
    const avg = nums.reduce((a, b) => a + b, 0) / nums.length;
    return `${label}: ${avg.toFixed(2)}`;
  }
  results.push(average());
  results.push(average('Score'));
  results.push(average('Grades', 90, 85, 92));
  results.push(average('Prices', 10.5, 20.75, 15.0));

  // Destructuring with defaults in params
  function processPoint({ x = 0, y = 0 }: { x?: number; y?: number } = {}): string {
    return `Point(${x}, ${y})`;
  }
  results.push(processPoint());
  results.push(processPoint({ x: 10 }));
  results.push(processPoint({ y: 20 }));
  results.push(processPoint({ x: 5, y: 15 }));

  // Arrow function with default params
  const multiply = (a: number = 1, b: number = 1): number => a * b;
  results.push(`multiply(): ${multiply()}`);
  results.push(`multiply(3): ${multiply(3)}`);
  results.push(`multiply(3, 4): ${multiply(3, 4)}`);

  // Rest parameters with spread in call
  const nums = [1, 2, 3];
  results.push(`sum(...[4, 5, 6]): ${sum(...nums)}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Function Params Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
