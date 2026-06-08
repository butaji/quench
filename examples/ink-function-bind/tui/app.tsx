// ink-function-bind example — bind, call, apply
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

function greet(greeting: string): string {
  return greeting + ", World!";
}

function sum(a: number, b: number, c: number): number {
  return a + b + c;
}

export default function App() {
  // Use inline expressions for bind, call, apply
  const greetAlice = greet.bind(null, "Hey");
  const callResult = greet.call(null, "Hello");
  const applyResult = greet.apply(null, ["Hi"]);
  const partial = sum.bind(null, 1);

  return (
    <Box flexDirection="column" padding={1}>
      <Text>Bind: {greetAlice()}</Text>
      <Text>Call: {callResult}</Text>
      <Text>Apply: {applyResult}</Text>
      <Text>Partial: {partial(2, 3)}</Text>
    </Box>
  );
}
