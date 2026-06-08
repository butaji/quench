// ink-asserts-predicate example — demonstrates `asserts` type predicates.
//
// `asserts value is Type` is a TypeScript assertion that:
// 1. Narrows the type if the assertion passes
// 2. Throws if the assertion fails
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: The `asserts` keyword is erased at compile time. The function
// body remains as runtime code.

import React from 'react';
import { Box, Text } from 'ink';

// --- Assert predicates ---
function assertIsString(value: unknown): asserts value is string {
  if (typeof value !== 'string') {
    throw new TypeError('Expected string, got ' + typeof value);
  }
}

function assertIsNumber(value: unknown): asserts value is number {
  if (typeof value !== 'number') {
    throw new TypeError('Expected number, got ' + typeof value);
  }
}

function assertNonNull<T>(value: T): asserts value is NonNullable<T> {
  if (value === null || value === undefined) {
    throw new TypeError('Expected non-null value');
  }
}

// --- Functions using assert predicates ---
function formatUpper(value: unknown): string {
  assertIsString(value);
  return value.toUpperCase();
}

function double(value: unknown): number {
  assertIsNumber(value);
  return value * 2;
}

function safeLength(value: unknown): number {
  assertNonNull(value);
  if (typeof value === 'string') {
    return value.length;
  }
  if (Array.isArray(value)) {
    return value.length;
  }
  return 0;
}

// --- Conditional asserts ---
function assertIsDefined<T>(value: T | undefined): asserts value is T {
  if (value === undefined) {
    throw new Error('Value is undefined');
  }
}

function assertIsArray<T>(value: unknown): asserts value is T[] {
  if (!Array.isArray(value)) {
    throw new Error('Expected array');
  }
}

// --- Usage ---
const stringValue: unknown = 'hello world';
const numberValue: unknown = 42;
const nullValue: unknown = null;
const arrayValue: unknown = [1, 2, 3];

export default function App() {
  const results: string[] = [];

  // Format strings
  results.push('String operations:');
  results.push('  formatUpper("hello world") = ' + formatUpper(stringValue));

  // Format numbers
  results.push('');
  results.push('Number operations:');
  results.push('  double(42) = ' + double(numberValue));

  // Safe operations
  results.push('');
  results.push('Safe operations:');
  results.push('  safeLength("test") = ' + safeLength('test'));
  results.push('  safeLength([1,2,3]) = ' + safeLength(arrayValue));

  // Using assertIsDefined
  const maybeString: string | undefined = 'defined';
  assertIsDefined(maybeString);
  results.push('');
  results.push('Defined check:');
  results.push('  after assert: "' + maybeString + '"');

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">asserts Type Predicate Demo</Text>
      <Text dimColor>asserts value is Type narrows type after check</Text>
      <Text></Text>
      {results.map((line, i) => (
        <Text key={i}>{line}</Text>
      ))}
    </Box>
  );
}
