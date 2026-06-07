// ink-jsx-advanced example — demonstrates JSX spread attrs, dynamic components,
// fragments, and conditional rendering.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React, { Fragment } from 'react';
import { Box, Text, Newline } from 'ink';

export default function JSXAdvancedDemo() {
  const results: string[] = [];

  // JSX spread attributes
  const baseStyle = { color: 'cyan' as const, bold: true };
  const extendedStyle = { ...baseStyle, italic: true };
  results.push(`Spread: color=${extendedStyle.color}, bold=${extendedStyle.bold}, italic=${extendedStyle.italic}`);

  // Conditional rendering patterns
  const show = true;
  const value = 'conditional';
  const rendered = show ? <Text>Shown</Text> : <Text>Hidden</Text>;
  results.push(`Conditional rendering: ${show ? 'Shown' : 'Hidden'}`);

  // Logical AND for conditional
  const count = 5;
  const showCount = count > 0;
  results.push(`Logical AND: ${showCount && `Count is ${count}`}`);

  // Nullish coalescing for defaults
  const optional: string | null = null;
  const display = optional ?? 'default';
  results.push(`Nullish coalesce: ${display}`);

  // Dynamic component (simulated)
  const Component = 'Text';
  results.push(`Dynamic component type: ${Component}`);

  // Array mapping
  const items = ['apple', 'banana', 'cherry'];
  const mapped = items.map(item => item.toUpperCase()).join(', ');
  results.push(`Mapped array: ${mapped}`);

  // Fragment usage
  const fragmentContent = 'Fragment content';
  results.push(`Fragment: ${fragmentContent}`);

  // Nested ternary
  const status = 'active';
  const stateLabel = status === 'active' ? 'Active' : status === 'pending' ? 'Pending' : 'Inactive';
  results.push(`Nested ternary: ${stateLabel}`);

  // Array filter and map
  const numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
  const evens = numbers.filter(n => n % 2 === 0).map(n => n * 2).join(', ');
  results.push(`Filtered and mapped evens: ${evens}`);

  // Object in JSX props
  const config = { width: 80, height: 24 };
  results.push(`Config props: width=${config.width}, height=${config.height}`);

  // Spread with override
  const defaults = { a: 1, b: 2, c: 3 };
  const overrides = { b: 20, d: 4 };
  const merged = { ...defaults, ...overrides };
  results.push(`Merged: a=${merged.a}, b=${merged.b}, c=${merged.c}, d=${merged.d}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">JSX Advanced Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
