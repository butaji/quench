// Input hook example — exercises the `useInput`
// hook for keyboard input handling. Shows a
// counter that increments on Enter and exits
// on q.
//
// Note: this example uses state which only
// works in the dev path (rquickjs evaluates
// it as JS) and deno. The build path doesn't
// evaluate JS expressions, so the counter
// won't update in the build output.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';
import { useState } from 'react';

export default function InputHook() {
  const [count, setCount] = useState(0);
  const [done, setDone] = useState(false);
  if (done) return null;
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Counter: {count}</Text>
      <Text dimColor>Press Enter to increment, q to quit</Text>
    </Box>
  );
}
