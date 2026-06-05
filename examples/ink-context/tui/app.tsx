// Context example — demonstrates React Context with Ink.
// Shows how to use context to pass data through the component tree.
//
// NOTE: React Context may not work in runts dev mode (HIR runtime).
// This version shows static content for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink) - full React Context support
//   2. runts dev (HIR runtime) - static render
//   3. runts build (codegen->runts-ink) - full interactivity

import React from 'react';
import { Box, Text } from 'ink';

// Theme values for demonstration (static for parity)
const primaryColor = 'cyan';
const secondaryColor = 'green';

function ThemeDisplay() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Context Demo</Text>
      <Text></Text>
      <Text>Primary Color: cyan</Text>
      <Text>Secondary Color: green</Text>
      <Text></Text>
      <Text dimColor>Context passes data without prop drilling.</Text>
    </Box>
  );
}

export default function ContextDemo() {
  return (
    <Box>
      <ThemeDisplay />
    </Box>
  );
}
