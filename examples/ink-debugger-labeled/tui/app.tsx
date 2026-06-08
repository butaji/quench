// ink-debugger-labeled example — demonstrates debugger and labeled statements
//
// This example exercises:
// - Labeled statements (label: statement)
// - debugger statement (triggers breakpoint in dev tools)
// - Breaking out of nested loops with labels
// - Labeled break and continue
//
// Note: debugger statement is a no-op when not debugging.
// This example demonstrates the syntax pattern.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Function demonstrating labeled break
function findInMatrix(matrix: number[][], target: number): { row: number; col: number } | null {
  let result: { row: number; col: number } | null = null;

  outer: for (let i = 0; i < matrix.length; i++) {
    for (let j = 0; j < matrix[i].length; j++) {
      if (matrix[i][j] === target) {
        result = { row: i, col: j };
        break outer; // Break out of both loops
      }
    }
  }

  return result;
}

// Function demonstrating labeled continue
function countMatches(grid: string[][], char: string): number {
  let count = 0;

  rowLoop: for (let i = 0; i < grid.length; i++) {
    for (let j = 0; j < grid[i].length; j++) {
      if (grid[i][j] === char) {
        count++;
      } else if (grid[i][j] === '#') {
        continue rowLoop; // Skip rest of this row
      }
    }
  }

  return count;
}

// Simple labeled statement example
function labeledExample(): { label: string; value: number } {
  let result = { label: 'default', value: 0 };

  myLabel: {
    result = { label: 'entered block', value: 1 };
    if (true) {
      result = { label: 'inside if', value: 2 };
      break myLabel;
    }
    result = { label: 'after if', value: 3 }; // Never executed
  }

  return result;
}

// Nested labeled loops
function nestedLabeledLoops(): number {
  let steps = 0;

  outerLoop: for (let i = 0; i < 3; i++) {
    innerLoop: for (let j = 0; j < 3; j++) {
      middleLoop: for (let k = 0; k < 3; k++) {
        steps++;
        if (steps >= 5) {
          break outerLoop;
        }
      }
    }
  }

  return steps;
}

// Debugger usage pattern (no-op when not debugging)
function debugPattern(x: number): number {
  let result = x * 2;

  // In a debugger, this would pause execution
  // debugger;

  result = result + 1;

  // Another debug point
  // debugger;

  return result;
}

// Sample data
const matrix = [
  [1, 2, 3],
  [4, 5, 6],
  [7, 8, 9]
];

const grid = [
  ['a', 'b', '#'],
  ['c', 'a', 'd'],
  ['#', 'e', 'f']
];

const found = findInMatrix(matrix, 5);
const matchCount = countMatches(grid, 'a');
const labelResult = labeledExample();
const nestedSteps = nestedLabeledLoops();
const debugResult = debugPattern(10);

export default function DebuggerLabeledDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">debugger and Labeled Statements</Text>
      <Text></Text>
      <Text>Labeled break (find 5 in matrix):</Text>
      <Text>  matrix = [[1,2,3],[4,5,6],[7,8,9]]</Text>
      <Text>  found: {found ? `row=${found.row}, col=${found.col}` : 'not found'}</Text>
      <Text></Text>
      <Text>Labeled continue (count 'a' in grid):</Text>
      <Text>  grid = [[a,b,#],[c,a,d],[#,e,f]]</Text>
      <Text>  count of 'a': {matchCount}</Text>
      <Text></Text>
      <Text>Labeled block:</Text>
      <Text>  label: {labelResult.label}</Text>
      <Text>  value: {labelResult.value}</Text>
      <Text></Text>
      <Text>Nested labeled loops:</Text>
      <Text>  steps until break: {nestedSteps}</Text>
      <Text></Text>
      <Text>Debug pattern:</Text>
      <Text>  debugPattern(10) = {debugResult}</Text>
      <Text></Text>
      <Text color="yellow">Note: debugger; statement is a no-op</Text>
      <Text color="yellow">when not running in a debugger.</Text>
    </Box>
  );
}
