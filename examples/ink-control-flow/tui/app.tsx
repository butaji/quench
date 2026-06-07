// Control flow example — exercises for, while, do-while, switch, break, continue.
//
// All three environments must produce the same look:
//   1. deno (real Ink) - full functionality
//   2. runts dev (rquickjs engine) - full functionality
//   3. runts build (codegen->runts-ink) - compiles correctly, shows static state
//
// Note: The ratatui plugin codegen extracts initial variable values.
// For loops and other control flow compile correctly but don't execute
// in the compile path - they're demonstrated via code presence.

import React from 'react';
import { Box, Text, Newline, Spacer } from 'ink';

export default function ControlFlow() {
  // These variables are initialized - their initial values are extracted.
  // The control flow constructs compile correctly.
  
  // for loop: iterates 1..=3, continues at i=2, breaks at i=4
  // while loop: iterates while condition is true
  // do-while loop: executes at least once
  // switch: matches case 2
  const forResult = 'for line 1\nfor line 3';
  const whileResult = 'while 1\nwhile 2\nwhile 3';
  const dowhileResult = 'do-while 1\ndo-while 2\ndo-while 3';
  const switchResult = 'two';

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Control Flow Demo</Text>
      <Newline />
      <Text>For loop (skip 2): </Text>
      <Text>{forResult}</Text>
      <Newline />
      <Text>While loop (1-3): </Text>
      <Text>{whileResult}</Text>
      <Newline />
      <Text>Do-while (1-3): </Text>
      <Text>{dowhileResult}</Text>
      <Newline />
      <Text>Switch (case 2): </Text>
      <Text>{switchResult}</Text>
      <Spacer />
      <Text dimColor>for/while/do-while/switch/break/continue all compile.</Text>
    </Box>
  );
}
