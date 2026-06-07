// ink-spread example — demonstrates object/array spread and JSX spread attributes.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Props type for demonstration
interface TextProps {
  color?: string;
  bold?: boolean;
  dimColor?: boolean;
}

export default function SpreadDemo() {
  const results: string[] = [];

  // Object spread in literals
  const base = { color: 'cyan', bold: true };
  const extended = { ...base, dimColor: true };
  results.push(`base: color=${base.color}, bold=${base.bold}`);
  results.push(`extended: color=${extended.color}, bold=${extended.bold}, dimColor=${extended.dimColor}`);

  // Array spread
  const arr1 = ['a', 'b', 'c'];
  const arr2 = [...arr1, 'd', 'e'];
  const combined = ['start', ...arr1, 'middle', ...arr2, 'end'];
  results.push(`arr1: ${arr1.join(', ')}`);
  results.push(`arr2: ${arr2.join(', ')}`);
  results.push(`combined: ${combined.join(', ')}`);

  // Nested object spread
  const nested = {
    level1: {
      level2: {
        value: 42,
      },
    },
  };
  const nestedSpread = { ...nested.level1 };
  results.push(`nested spread: ${nestedSpread.level2.value}`);

  // Array with primitive spread
  const nums = [1, 2, 3];
  const moreNums = [4, 5, 6];
  const allNums = [...nums, ...moreNums];
  results.push(`nums sum: ${allNums.reduce((a, b) => a + b, 0)}`);

  // Object spread with override
  const defaults = { color: 'white', bold: false, italic: false };
  const userPrefs = { bold: true, italic: true };
  const styles = { ...defaults, ...userPrefs };
  results.push(`styles: color=${styles.color}, bold=${styles.bold}, italic=${styles.italic}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Spread Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
