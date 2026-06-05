// Advanced Table Example — demonstrates table rendering.
// Shows tabular data with columns and alignment.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)
//
// NOTE: Custom components and string repeat are not supported in runts HIR runtime.

import React from 'react';
import { Box, Text } from 'ink';

export default function TableAdvanced() {
  // Static values for parity testing
  const data = [
    { name: "Alice", score: 92, grade: "A" },
    { name: "Bob", score: 78, grade: "C" },
    { name: "Charlie", score: 85, grade: "B" },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Student Scores</Text>
      <Text></Text>
      <Box width={50} justifyContent="space-between">
        <Text bold>Name</Text>
        <Text bold>Score</Text>
        <Text bold>Grade</Text>
      </Box>
      <Box width={50} justifyContent="space-between">
        <Text dimColor>---------</Text>
        <Text dimColor>------</Text>
        <Text dimColor>-----</Text>
      </Box>
      <Box width={50} justifyContent="space-between">
        <Text>Alice</Text>
        <Text>92</Text>
        <Text color="green">A</Text>
      </Box>
      <Box width={50} justifyContent="space-between">
        <Text>Bob</Text>
        <Text>78</Text>
        <Text color="yellow">C</Text>
      </Box>
      <Box width={50} justifyContent="space-between">
        <Text>Charlie</Text>
        <Text>85</Text>
        <Text color="green">B</Text>
      </Box>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
