// Conditional rendering example — exercises JSX
// conditional expressions ({condition ? 'yes' : 'no'}).
//
// Renders a column box with text content that
// uses ternary operators to show/hide values.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';
import React from 'react';

export default function App() {
  const isActive = true;
  const count = 3;
  const items = ['first', 'second', 'third'];
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="round">
      <Text color={isActive ? 'green' : 'red'}>
        Status: {isActive ? 'ACTIVE' : 'INACTIVE'}
      </Text>
      <Text>Count: {count}</Text>
      <Text>Item 1: {items[0]}</Text>
      <Text>Item 2: {items[1]}</Text>
      <Text>Item 3: {items[2]}</Text>
    </Box>
  );
}
