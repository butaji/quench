// Control Flow example — demonstrates for, while, do-while, switch, break, continue.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: The compile path has limited expression support. The dev path (rquickjs)
// demonstrates full TS/TSX execution. The compile path demonstrates structure.

import React from 'react';
import { Box, Text } from 'ink';

export default function ControlFlowDemo() {
  // These demonstrate TypeScript control flow constructs.
  // The dev path executes them via rquickjs.
  
  // for loop
  let forCount = 0;
  for (let i = 0; i < 3; i++) {
    forCount = forCount + 1;
  }

  // while loop
  let whileCount = 0;
  let w = 0;
  while (w < 3) {
    whileCount = whileCount + 1;
    w = w + 1;
  }

  // do-while loop
  let doWhileCount = 0;
  let d = 0;
  do {
    doWhileCount = doWhileCount + 1;
    d = d + 1;
  } while (d < 3);

  // switch
  let switchResult = 0;
  const switchValue = 2;
  switch (switchValue) {
    case 1:
      switchResult = 1;
      break;
    case 2:
      switchResult = 2;
      break;
    case 3:
      switchResult = 3;
      break;
    default:
      switchResult = 99;
  }

  // break/continue (simplified)
  let breakContinueResult = 0;
  let b = 0;
  while (b < 5) {
    if (b === 4) {
      break;
    }
    if (b === 2) {
      b = b + 1;
      continue;
    }
    breakContinueResult = breakContinueResult + 1;
    b = b + 1;
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Control Flow Demo</Text>
      <Text></Text>
      <Text dimColor>for loop (count=3):</Text>
      <Text>{forCount}</Text>
      <Text dimColor>while loop (count=3):</Text>
      <Text>{whileCount}</Text>
      <Text dimColor>do-while loop (count=3):</Text>
      <Text>{doWhileCount}</Text>
      <Text dimColor>switch (value=2):</Text>
      <Text>{switchResult}</Text>
      <Text dimColor>break/continue:</Text>
      <Text>{breakContinueResult}</Text>
    </Box>
  );
}
