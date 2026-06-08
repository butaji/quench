// ink-object-modern example — demonstrates modern ES2022+ object methods.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Modern object methods are standard JavaScript runtime features.

import React from 'react';
import { Box, Text } from 'ink';

// --- Object.fromEntries (ES2019) ---
const pairs: [string, number | string][] = [
  ['x', 10],
  ['y', 20],
  ['name', 'point'],
];
const point = Object.fromEntries(pairs);

// --- Object.hasOwn (ES2022) ---
const target = { x: 1, y: 2, nested: { z: 3 } };
const hasX = Object.hasOwn(target, 'x');
const hasZ = Object.hasOwn(target, 'z');
const hasToString = Object.hasOwn(target, 'toString');
const hasValueOf = Object.hasOwn(target, 'valueOf');

// --- Object.getOwnPropertyDescriptors (ES2017) ---
const desc = Object.getOwnPropertyDescriptors(target);

// --- Object.keys, values, entries (ES2017 baseline) ---
const obj = { a: 1, b: 2, c: 3 };

// --- Object.assign (baseline) ---
const merged = Object.assign({}, { x: 1 }, { y: 2 }, { x: 3 });

// --- Object.freeze (baseline) ---
const frozen = Object.freeze({ value: 42 });
// @ts-expect-error - frozen object (but still accessible in JS runtime)

// --- Object.seal (baseline) ---
const sealed = Object.seal({ value: 43 });

// --- Object.is (ES2015) ---
const isSame = Object.is(NaN, NaN);
const isPositiveZero = Object.is(+0, -0);
const isEqual = Object.is('foo', 'foo');

// --- Object.create (baseline) ---
const proto = { greeting: 'hello' };
const child = Object.create(proto);
child.name = 'World';

// --- Object.defineProperty (baseline) ---
const definee: any = {};
Object.defineProperty(definee, 'computed', {
  value: 100,
  writable: false,
  enumerable: true,
  configurable: false,
});

// --- Object.getPrototypeOf / setPrototypeOf ---
const proto2 = { base: true };
const instance: any = { derived: true };
const protoOf = Object.getPrototypeOf(instance);

// --- Object.values, Object.entries ---
const mixed = { name: 'Alice', age: 30, city: 'NYC' };

// --- Immutable Object methods ---
const toStringified = Object.toStringified?.(mixed) || JSON.stringify(mixed);

export default function ObjectModernDemo() {
  const results: string[] = [];

  // fromEntries
  results.push(`fromEntries.x: ${(point as any).x}`);
  results.push(`fromEntries.name: ${(point as any).name}`);

  // hasOwn
  results.push(`hasOwn('x'): ${hasX}`);
  results.push(`hasOwn('z'): ${hasZ}`);
  results.push(`hasOwn('toString'): ${hasToString}`);
  results.push(`hasOwn('valueOf'): ${hasValueOf}`);

  // getOwnPropertyDescriptors
  results.push(`descriptors keys: ${Object.keys(desc).join(', ')}`);
  results.push(`desc.x.writable: ${desc.x?.writable}`);
  results.push(`desc.y.value: ${desc.y?.value}`);

  // keys, values, entries
  results.push(`keys: ${Object.keys(obj).join(', ')}`);
  results.push(`values: ${Object.values(obj).join(', ')}`);
  results.push(`entries: ${Object.entries(obj).map(([k, v]) => `${k}=${v}`).join(', ')}`);

  // assign
  results.push(`assign: ${Object.entries(merged).map(([k, v]) => `${k}=${v}`).join(', ')}`);

  // is
  results.push(`Object.is(NaN, NaN): ${isSame}`);
  results.push(`Object.is(+0, -0): ${isPositiveZero}`);
  results.push(`Object.is('foo', 'foo'): ${isEqual}`);

  // create
  results.push(`create.greeting: ${(child as any).greeting}`);
  results.push(`create.name: ${(child as any).name}`);

  // defineProperty
  results.push(`defineProperty: ${definee.computed}`);

  // getPrototypeOf
  results.push(`getPrototypeOf: ${protoOf?.base ?? protoOf?.constructor?.name}`);

  // Object.values/entries
  results.push(`values: ${Object.values(mixed).join(', ')}`);
  results.push(`entries: ${Object.entries(mixed).map(([k, v]) => `${k}:${v}`).join(', ')}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Modern Object Methods Demo</Text>
      <Text dimColor>ES2015-ES2022 object features</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
