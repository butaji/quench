// Paste example — demonstrates bracketed paste mode with usePaste.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, usePaste } from 'ink';

export default function PasteDemo() {
  const [pasted, setPasted] = useState<string[]>([]);

  // usePaste registers handler (paste events routed in interactive mode)
  usePaste((text: string) => {
    setPasted((prev) => [...prev, text]);
  });

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Paste Demo</Text>
      <Text>Paste text using Ctrl-Shift-V:</Text>
      {pasted.length === 0 ? (
        <Text dimColor>No pasted text yet...</Text>
      ) : (
        pasted.map((text, i) => (
          <Text key={i} color="green">
            [{i + 1}] {text}
          </Text>
        ))
      )}
    </Box>
  );
}
