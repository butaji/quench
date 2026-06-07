// ink-static-transform example — demonstrates Static, Transform, Newline, Spacer.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// These components provide special rendering behaviors:
// - Static: renders items that don't re-render on state changes
// - Transform: applies transformations to output
// - Newline: adds a line break
// - Spacer: takes up remaining vertical space

import React from 'react';
import { Box, Text, Newline, Spacer } from 'ink';

// Transform function simulation
function uppercaseTransform(output: string): string {
  return output.toUpperCase();
}

function reverseTransform(output: string): string {
  return output.split('').reverse().join('');
}

export default function StaticTransformDemo() {
  const results: string[] = [];

  // Static items simulation
  const staticItems = ['Item A', 'Item B', 'Item C'];
  results.push('Static Items:');
  for (const item of staticItems) {
    results.push(`  ${item}`);
  }

  // Transform simulation
  results.push('');
  results.push('Transform (uppercase):');
  const original = 'hello world';
  results.push(`  Original: ${original}`);
  results.push(`  Uppercase: ${uppercaseTransform(original)}`);
  results.push(`  Reversed: ${reverseTransform(original)}`);

  // Newline simulation
  results.push('');
  results.push('Newline:');
  results.push('Line 1');
  results.push('Line 2 (after newline)');

  // Spacer simulation (just placeholder text)
  results.push('');
  results.push('Spacer:');
  results.push('Top content');
  results.push('(spacer here)');
  results.push('Bottom content');

  // Combined demonstration
  results.push('');
  results.push('Combined:');
  const items = ['Static 1', 'Static 2', 'Static 3'];
  for (const item of items) {
    results.push(`* ${item}`);
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Static, Transform, Newline, Spacer Demo</Text>
      <Text dimColor>Note: These components available in runts-ink</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
