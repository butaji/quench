// ink-unknown-never example — demonstrates unknown, never, and user-defined type guards.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Type-level constructs are erased at compile time. Type guards
// become regular boolean checks at runtime.

import React from 'react';
import { Box, Text } from 'ink';

// --- User-defined type guards ---
function isString(x: unknown): x is string {
  return typeof x === 'string';
}

function isNumber(x: unknown): x is number {
  return typeof x === 'number';
}

function isBoolean(x: unknown): x is boolean {
  return typeof x === 'boolean';
}

function isDate(x: unknown): x is Date {
  return x instanceof Date;
}

// --- Type guard with custom predicate ---
interface Cat {
  kind: 'cat';
  meow: () => string;
}
interface Dog {
  kind: 'dog';
  bark: () => string;
}
type Pet = Cat | Dog;

function isCat(p: unknown): p is Cat {
  return typeof p === 'object' && p !== null && (p as Pet).kind === 'cat';
}

// --- Format using type guards ---
function formatValue(x: unknown): string {
  if (isString(x)) {
    return `str: "${x.toUpperCase()}"`;
  }
  if (isNumber(x)) {
    return `num: ${x.toFixed(2)}`;
  }
  if (isBoolean(x)) {
    return `bool: ${x ? 'yes' : 'no'}`;
  }
  if (isDate(x)) {
    return `date: ${x.toISOString()}`;
  }
  if (isCat(x)) {
    return `pet: cat (says "${x.meow()}")`;
  }
  return `unknown: ${String(x)}`;
}

// --- never type for exhaustive checks ---
type Status = 'idle' | 'loading' | 'success' | 'error';

function assertNever(x: never): never {
  throw new Error(`Unexpected value: ${JSON.stringify(x)}`);
}

function getStatusLabel(s: Status): string {
  switch (s) {
    case 'idle': return 'Status: Idle';
    case 'loading': return 'Status: Loading...';
    case 'success': return 'Status: Done!';
    case 'error': return 'Status: Failed!';
    default: return assertNever(s);
  }
}

// --- Function returning never ---
function fail(message: string): never {
  throw new Error(message);
}

// Conditional never (never is assignable to everything, nothing to never)
function processValue(x: string | number): string {
  if (typeof x === 'string') {
    return `string: ${x.length}`;
  } else if (typeof x === 'number') {
    return `number: ${x}`;
  }
  // At this point, x is never
  return assertNever(x);
}

// --- Values to format ---
const values: unknown[] = [
  'hello',
  42,
  3.14159,
  true,
  new Date('2024-01-15'),
  { kind: 'cat', meow: () => 'meow!' } as Cat,
  null,
  undefined,
  { random: 'object' },
];

export default function UnknownNeverDemo() {
  const results: string[] = [];

  // Status labels (never for exhaustive)
  results.push(getStatusLabel('idle'));
  results.push(getStatusLabel('loading'));
  results.push(getStatusLabel('success'));
  results.push(getStatusLabel('error'));

  results.push('');

  // Format all values
  for (const v of values) {
    results.push(formatValue(v));
  }

  results.push('');

  // Process values
  results.push(processValue('test'));
  results.push(processValue(123));

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">unknown, never & Type Guards Demo</Text>
      <Text dimColor>Type guards erased to runtime checks</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
