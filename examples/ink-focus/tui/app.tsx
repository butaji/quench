// Focus example — exercises `useFocus` and
// `useFocusManager` hooks for keyboard
// navigation between focusable elements.
// Press Tab to cycle focus, Enter to select.
//
// Note: this example uses state which only
// works in the dev path (rquickjs evaluates
// it as JS) and deno. The build path doesn't
// evaluate JS expressions, so the focus
// indicator won't update in the build output.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs+bridge)
//   3. runts build (codegen->runts-ink)

import { Box, Text } from 'ink';
import { useState } from 'react';

export default function Focus() {
  const [active, setActive] = useState(0);
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Tab to cycle focus:</Text>
      <Box flexDirection="row" marginTop={1}>
        <Text color={active === 0 ? 'green' : undefined}>{'[1] Button A'}</Text>
        <Text color={active === 1 ? 'green' : undefined}>{'  [2] Button B'}</Text>
        <Text color={active === 2 ? 'green' : undefined}>{'  [3] Button C'}</Text>
      </Box>
    </Box>
  );
}
