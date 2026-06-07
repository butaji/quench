// ink-compound-bitwise example — demonstrates all compound assignment and bitwise operators.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function CompoundBitwiseDemo() {
  const results: string[] = [];

  // All compound assignment operators
  let a = 10;
  a += 5;  // 15
  results.push(`a += 5: ${a}`);

  let b = 20;
  b -= 3;  // 17
  results.push(`b -= 3: ${b}`);

  let c = 4;
  c *= 3;  // 12
  results.push(`c *= 3: ${c}`);

  let d = 20;
  d /= 4;  // 5
  results.push(`d /= 4: ${d}`);

  let e = 17;
  e %= 5;  // 2
  results.push(`e %= 5: ${e}`);

  let f = 2;
  f **= 3;  // 8
  results.push(`f **= 3: ${f}`);

  let g = 5;
  g <<= 2;  // 20
  results.push(`g <<= 2: ${g}`);

  let h = 20;
  h >>= 2;  // 5
  results.push(`h >>= 2: ${h}`);

  let i = -20;
  i >>>= 2;  // 1073741819 (unsigned right shift)
  results.push(`i >>>= 2: ${i >>> 0}`);

  let j = 5;
  j |= 3;  // 7
  results.push(`j |= 3: ${j}`);

  let k = 6;
  k &= 3;  // 2
  results.push(`k &= 3: ${k}`);

  let l = 6;
  l ^= 3;  // 5
  results.push(`l ^= 3: ${l}`);

  // Bitwise operators
  const x = 5;   // 0101
  const y = 3;   // 0011
  results.push(`x = 5, y = 3`);
  results.push(`x | y: ${x | y}`);   // 7 (0101 | 0011 = 0111)
  results.push(`x & y: ${x & y}`);   // 1 (0101 & 0011 = 0001)
  results.push(`x ^ y: ${x ^ y}`);   // 6 (0101 ^ 0011 = 0110)
  results.push(`~x: ${~x}`);         // -6

  // Shift operators
  results.push(`x << 1: ${x << 1}`);  // 10
  results.push(`x >> 1: ${x >> 1}`);  // 2
  results.push(`x >>> 1: ${x >>> 1}`); // 2

  // Combined operations
  let n = 8;
  n |= 1;  // 9
  n &= ~1; // 8 (clear last bit)
  results.push(`bit toggle: ${n}`);

  // String concatenation operator
  let str = 'Hello';
  str += ', ';
  str += 'World!';
  results.push(`string concat: ${str}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Compound & Bitwise Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
