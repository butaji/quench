// ink-ts-directive example — demonstrates TypeScript compiler directives.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: TypeScript directives are comments that are erased at compile time.
// They have no runtime impact.

import React from 'react';
import { Box, Text } from 'ink';

export default function TsDirectiveDemo() {
  // Type assertion with ts-ignore
  // @ts-ignore — intentionally ignoring type error
  const ignoredValue = 'hello' as unknown as number;

  // Type assertion with ts-expect-error
  // @ts-expect-error — this should error but we're testing the directive
  const expectErrorValue = 42 as unknown as string;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">TypeScript Directives Demo</Text>
      <Text dimColor>Directives are comments, erased at runtime</Text>
      <Text></Text>
      <Text>Ignored: {ignoredValue}</Text>
      <Text>ExpectError: {expectErrorValue}</Text>
      <Text></Text>
      <Text dimColor>Directives used:</Text>
      <Text>  // @ts-ignore</Text>
      <Text>  // @ts-expect-error</Text>
    </Box>
  );
}
