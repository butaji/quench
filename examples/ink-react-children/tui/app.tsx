// ink-react-children example — demonstrates Children API, cloneElement, isValidElement.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Children.count, Children.map, Children.forEach, Children.only, Children.toArray
// are React runtime features available in the React shim.

import React, { Children } from 'react';
import { Box, Text } from 'ink';

function ItemList({ children }: { children: React.ReactNode }) {
  const count = Children.count(children);
  return (
    <Box flexDirection="column">
      <Text>Count: {count}</Text>
      <Box flexDirection="column">{children}</Box>
    </Box>
  );
}

export default function App() {
  return (
    <ItemList>
      <Text>Apple</Text>
      <Text>Banana</Text>
      <Text>Cherry</Text>
    </ItemList>
  );
}
