// useLayoutEffect example — demonstrates synchronous layout effect hook
//
// useLayoutEffect runs synchronously before the browser paints.
// In Ink context, it's useful for measuring terminal dimensions
// before the first render to avoid visual flicker.

import React, { useLayoutEffect, useState, useRef } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  const [layoutRan, setLayoutRan] = useState(false);
  const initialized = useRef(false);

  // useLayoutEffect runs synchronously before paint
  // This is the main use case in React: measure DOM before paint
  useLayoutEffect(() => {
    // Only run once - avoid infinite loop
    if (!initialized.current) {
      initialized.current = true;
      setLayoutRan(true);
    }
  }, []);

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>useLayoutEffect Demo</Text>
      <Text>Layout effect ran: {layoutRan ? 'yes' : 'no'}</Text>
      <Text dimColor>(useLayoutEffect runs before paint)</Text>
    </Box>
  );
}
