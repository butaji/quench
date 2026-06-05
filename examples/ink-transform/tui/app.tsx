// Transform example — exercises the `<Transform>`
import React from 'react';
// component which applies a string transform
// function to its child's output.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text, Transform } from 'ink';

export default function TransformExample() {
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Transform transform={(output) => output.toUpperCase()}>
        <Text>uppercase transform</Text>
      </Transform>
      <Transform transform={(output) => `> ${output}`}>
        <Text>prefix transform</Text>
      </Transform>
      <Transform transform={(output) => output.split('').reverse().join('')}>
        <Text>reversed</Text>
      </Transform>
    </Box>
  );
}
