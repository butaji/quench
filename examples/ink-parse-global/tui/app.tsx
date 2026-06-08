// ink-parse-global example — demonstrates global parsing functions
//
// This example exercises the global parsing functions:
// - parseInt(string, radix?) - parses string to integer
// - parseFloat(string) - parses string to floating-point number
// - isNaN(value) - checks if value is NaN
// - isFinite(value) - checks if value is finite
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Format number for display
function fmt(n: number): string {
  return Number.isNaN(n) ? 'NaN' : String(n);
}

// parseInt with different radixes
const piStr = '3.14159';
const intFromFloat = parseInt(piStr, 10);
const hexStr = 'FF';
const hexValue = parseInt(hexStr, 16);
const binaryStr = '1010';
const binaryValue = parseInt(binaryStr, 2);
const octalStr = '77';
const octalValue = parseInt(octalStr, 8);

// parseFloat
const floatFromInt = parseFloat('42');
const scientificStr = '2.5e2';
const scientificValue = parseFloat(scientificStr);
const invalidNum = parseFloat('not a number');

// isNaN checks
const nanCheck = isNaN(NaN);
const zeroIsNan = isNaN(0);
const undefinedIsNan = isNaN(undefined as unknown);
const stringIsNan = isNaN('hello');
const nullIsNan = isNaN(null);

// isFinite checks
const finiteCheck = isFinite(42);
const infinityIsFinite = isFinite(Infinity);
const negInfinityIsFinite = isFinite(-Infinity);
const nanIsFinite = isFinite(NaN);
const undefinedIsFinite = isFinite(undefined as unknown);
const nullIsFinite = isFinite(null);

// Combined usage
function safeParseInt(s: string, radix?: number): number {
  const n = parseInt(s, radix);
  return isNaN(n) ? 0 : n;
}

function safeParseFloat(s: string): number {
  const n = parseFloat(s);
  return isFinite(n) ? n : 0;
}

const safeInt = safeParseInt('abc');
const safeFloat = safeParseFloat('xyz');

// Summary
const intExamples = [
  { input: '42', radix: 10, result: 42 },
  { input: '3.14', radix: 10, result: 3 },
  { input: 'FF', radix: 16, result: 255 },
  { input: '1010', radix: 2, result: 10 },
  { input: 'hello', radix: 10, result: NaN },
];

const floatExamples = [
  { input: '3.14', result: 3.14 },
  { input: '42', result: 42 },
  { input: '2.5e2', result: 250 },
  { input: 'hello', result: NaN },
];

export default function ParseGlobalDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">parseInt / parseFloat / isNaN / isFinite</Text>
      <Text></Text>
      <Text>parseInt examples:</Text>
      <Text>  parseInt('3.14', 10) = {intFromFloat}</Text>
      <Text>  parseInt('FF', 16) = {hexValue}</Text>
      <Text>  parseInt('1010', 2) = {binaryValue}</Text>
      <Text>  parseInt('77', 8) = {octalValue}</Text>
      <Text></Text>
      <Text>parseFloat examples:</Text>
      <Text>  parseFloat('42') = {floatFromInt}</Text>
      <Text>  parseFloat('2.5e2') = {scientificValue}</Text>
      <Text>  parseFloat('not a number') = {fmt(invalidNum)}</Text>
      <Text></Text>
      <Text>isNaN checks:</Text>
      <Text>  isNaN(NaN) = {nanCheck ? 'true' : 'false'}</Text>
      <Text>  isNaN(0) = {zeroIsNan ? 'true' : 'false'}</Text>
      <Text>  isNaN(undefined) = {undefinedIsNan ? 'true' : 'false'}</Text>
      <Text>  isNaN('hello') = {stringIsNan ? 'true' : 'false'}</Text>
      <Text>  isNaN(null) = {nullIsNan ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>isFinite checks:</Text>
      <Text>  isFinite(42) = {finiteCheck ? 'true' : 'false'}</Text>
      <Text>  isFinite(Infinity) = {infinityIsFinite ? 'true' : 'false'}</Text>
      <Text>  isFinite(-Infinity) = {negInfinityIsFinite ? 'true' : 'false'}</Text>
      <Text>  isFinite(NaN) = {nanIsFinite ? 'true' : 'false'}</Text>
      <Text>  isFinite(undefined) = {undefinedIsFinite ? 'true' : 'false'}</Text>
      <Text>  isFinite(null) = {nullIsFinite ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>Safe parsing:</Text>
      <Text>  safeParseInt('abc') = {safeInt}</Text>
      <Text>  safeParseFloat('xyz') = {safeFloat}</Text>
    </Box>
  );
}
