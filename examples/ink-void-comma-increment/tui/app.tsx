// ink-void-comma-increment example — demonstrates void, comma operator, ++ and --.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: These are standard JavaScript operators.

import React from 'react';
import { Box, Text } from 'ink';

// --- void operator ---
const voidResult = void 42;
const voidFn = void (console.log('void called'));

// --- comma operator ---
const commaResult = (1, 2, 3);
const commaWithAssignment = (() => {
  let x = 0;
  return (x++, x * 2); // increment then multiply
})();

// --- increment/decrement prefix ---
let a = 5;
const prefixInc = ++a; // increments first, returns new value
let b = 5;
const prefixDec = --b;
let c = 5;
const postfixInc = c++; // returns old value, then increments
let d = 5;
const postfixDec = d--;

// --- combined expressions ---
const combined = (1, 2, 3) + (4, 5, 6);

export default function VoidCommaIncrementDemo() {
  const results: string[] = [];

  // void
  results.push(`void 42: ${voidResult}`);
  results.push(`void 'expr': ${voidResult}`);

  results.push('');

  // comma operator
  results.push(`(1, 2, 3): ${commaResult}`);
  results.push(`(x++, x * 2) where x=0: ${commaWithAssignment}`);

  results.push('');

  // prefix increment/decrement
  results.push(`++a where a=5: ${prefixInc}`);
  results.push(`--b where b=5: ${prefixDec}`);

  // postfix increment/decrement
  results.push(`c++ where c=5: ${postfixInc}`);
  results.push(`d-- where d=5: ${postfixDec}`);

  results.push('');

  // combined
  results.push(`(1, 2, 3) + (4, 5, 6): ${combined}`);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Void/Comma/Increment Demo</Text>
      <Text dimColor>void operator, comma operator, ++ and --</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
