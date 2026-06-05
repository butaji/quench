// Advanced Table Example — demonstrates table rendering.
// Shows tabular data with columns and alignment.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

interface TableRow {
  name: string;
  score: number;
  grade: string;
}

function TableHeader() {
  return (
    <Box width={50} justifyContent="space-between">
      <Text bold>Name</Text>
      <Text bold>Score</Text>
      <Text bold>Grade</Text>
    </Box>
  );
}

function TableDivider() {
  return (
    <Box width={50} justifyContent="space-between">
      <Text dimColor>{"─".repeat(10)}</Text>
      <Text dimColor>{"─".repeat(6)}</Text>
      <Text dimColor>{"─".repeat(5)}</Text>
    </Box>
  );
}

function TableDataRow({ row }: { row: TableRow }) {
  const gradeColor = row.score >= 80 ? "green" : row.score >= 60 ? "yellow" : "red";
  return (
    <Box width={50} justifyContent="space-between">
      <Text>{row.name}</Text>
      <Text>{row.score}</Text>
      <Text color={gradeColor}>{row.grade}</Text>
    </Box>
  );
}

export default function TableAdvanced() {
  // Static values for parity testing
  const data: TableRow[] = [
    { name: "Alice", score: 92, grade: "A" },
    { name: "Bob", score: 78, grade: "C" },
    { name: "Charlie", score: 85, grade: "B" },
  ];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Student Scores</Text>
      <Text></Text>
      <TableHeader />
      <TableDivider />
      {data.map((row, i) => (
        <TableDataRow key={i} row={row} />
      ))}
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
