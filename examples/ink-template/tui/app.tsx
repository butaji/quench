// ink-template example — demonstrates template literals and multiline strings.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function TemplateDemo() {
  const results: string[] = [];

  // Basic template literal
  const name = 'World';
  const greeting = `Hello, ${name}!`;
  results.push(greeting);

  // Expression interpolation
  const a = 10;
  const b = 20;
  results.push(`${a} + ${b} = ${a + b}`);

  // Nested template literals
  const inner = 'nested';
  const outer = `outer ${`${inner} template`}`;
  results.push(outer);

  // Array join with template
  const items = ['apple', 'banana', 'cherry'];
  results.push(`Fruits: ${items.map(f => f.toUpperCase()).join(', ')}`);

  // Object property access in template
  const user = { name: 'Alice', age: 30 };
  results.push(`${user.name} is ${user.age} years old`);

  // Function call in template
  function getDate(): string {
    return '2024-01-15';
  }
  results.push(`Date: ${getDate()}`);

  // Multiline template (using join to avoid rendering issues)
  const lines = ['Line 1', 'Line 2', 'Line 3'];
  results.push(`Lines: ${lines.join(' | ')}`);

  // Conditional in template
  const flag = true;
  results.push(`Flag is ${flag ? 'ON' : 'OFF'}`);

  // Template with escape sequences
  const escaped = `Backslash: \\, Backtick: \`, Dollar: \$`;
  results.push(escaped);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Template Literal Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
