// ink-const-type-param example — demonstrates `const` type parameters (TS 5.0).
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: `const` type parameters are erased at compile time.
// They have no runtime impact on the generated JavaScript or Rust code.

import React from 'react';
import { Box, Text } from 'ink';

// --- Const type parameter with tuple inference ---
function createTuple<const T extends readonly unknown[]>(...args: T): T {
  return args;
}

// When called with literals, T is inferred as a tuple of literals
const tuple1 = createTuple('a', 'b', 'c');
const tuple2 = createTuple(1, 2, 3);
const tuple3 = createTuple(true, false);

// --- Const type parameter with object inference ---
function createConfig<const T extends object>(config: T): T {
  return config;
}

const config1 = createConfig({ name: 'App', version: 1 });
const config2 = createConfig({ debug: true, port: 3000 });

// --- Const type parameter with array literal ---
function freezeArray<const T extends readonly string[]>(arr: T): ReadonlyArray<string> {
  return arr;
}

const frozen1 = freezeArray(['x', 'y', 'z']);
const frozen2 = freezeArray(['one', 'two']);

// --- Generic function with const type param ---
function pair<const K extends string, const V>(key: K, value: V): [K, V] {
  return [key, value];
}

const pair1 = pair('id', 123);
const pair2 = pair('name', 'Alice');

// --- Const type parameter preserves literal types ---
function identity<const T>(value: T): T {
  return value;
}

const str = identity('hello');
const num = identity(42);
const bool = identity(true);

export default function ConstTypeParamDemo() {
  const results: string[] = [];

  // Tuple results
  results.push(`Tuple1[0]: ${tuple1[0]}`);
  results.push(`Tuple1 length: ${tuple1.length}`);
  results.push(`Tuple2: ${tuple2.join(', ')}`);
  results.push(`Tuple3: ${tuple3.join(', ')}`);

  // Config results
  results.push(`Config1.name: ${config1.name}`);
  results.push(`Config1.version: ${config1.version}`);
  results.push(`Config2.debug: ${config2.debug}`);
  results.push(`Config2.port: ${config2.port}`);

  // Frozen array results
  results.push(`Frozen1[0]: ${frozen1[0]}`);
  results.push(`Frozen2 length: ${frozen2.length}`);

  // Pair results
  results.push(`Pair1[0]: ${pair1[0]}, Pair1[1]: ${pair1[1]}`);
  results.push(`Pair2[0]: ${pair2[0]}, Pair2[1]: ${pair2[1]}`);

  // Identity results
  results.push(`Str: ${str}`);
  results.push(`Num: ${num}`);
  results.push(`Bool: ${bool}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">const Type Parameters Demo (TS 5.0)</Text>
      <Text dimColor>Type-level only, erased at compile time</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
