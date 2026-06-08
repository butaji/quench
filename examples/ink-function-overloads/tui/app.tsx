// Function overloads example — exercises TypeScript function overloads.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen)

import React from 'react';
import { Box, Text } from 'ink';

function format(input: string): string;
function format(input: number): string;
function format(input: string | number): string {
  if (typeof input === 'string') {
    return input.toUpperCase();
  }
  return `Number: ${input}`;
}

class Formatter {
  format(input: string): string;
  format(input: number): string;
  format(input: string | number): string {
    return format(input);
  }
}

const fmt = new Formatter();

export default function App() {
  const s1 = format('hello');
  const s2 = format(42);
  const s3 = fmt.format('world');
  const s4 = fmt.format(99);

  return (
    <Box flexDirection="column">
      <Text>String: {s1}</Text>
      <Text>Number: {s2}</Text>
      <Text>Class string: {s3}</Text>
      <Text>Class number: {s4}</Text>
    </Box>
  );
}
