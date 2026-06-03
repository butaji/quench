// Bordered example — identical source runs in:
//   1. actual Ink (`deno run bordered.tsx` in /tmp/ink-reference)
//   2. runts (`runts build examples/ink-bordered --plugin ratatui`)
//
// Plain JSX. The runts-ratatui plugin's Ink tag dispatch
// (`Box` / `Text` / `Newline` / `Spacer`) lowers this to
// `runts_ink::Box::*` / `runts_ink::Text::*` calls; actual Ink
// uses the React reconciler on the same JSX.

import React from 'react';
import { render, Box, Text } from 'ink';

function App() {
  return (
    <Box flexDirection="column" borderStyle="round" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Bordered Example</Text>
      <Text>{''}</Text>
      <Text>A boxed card with a title, a blank</Text>
      <Text>line for breathing room, two body lines,</Text>
      <Text>and a hint at the bottom.</Text>
      <Text italic>Press q to quit.</Text>
    </Box>
  );
}

render(<App />);
