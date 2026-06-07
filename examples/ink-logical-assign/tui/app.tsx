// Logical assignment operators example — demonstrates ||=, &&=, and ??=
//
// ES2021 logical assignment operators:
//   ||= (logical OR assignment) — assign if falsy
//   &&= (logical AND assignment) — assign if truthy
//   ??= (nullish coalescing assignment) — assign if null/undefined

import React, { useState } from 'react';
import { Box, Text } from 'ink';

export default function App() {
  // Simulate props from parent (may be undefined)
  const [userName] = useState<string | undefined>(undefined);
  const [adminName] = useState<string | undefined>('Alice');
  const [settings] = useState<{theme?: string; debug?: boolean}>({});
  const [count] = useState(5);
  const [flag] = useState(true);
  const [value] = useState<number | null>(null);

  // ||=: assign if falsy (null, undefined, 0, '', false, NaN)
  let displayName = userName;
  displayName ||= 'Anonymous';

  // &&=: assign if truthy
  let status = count > 0 && 'active';
  let label = 'Status: ';
  label &&= status;

  // ??=: assign if nullish (null or undefined only)
  let theme = settings.theme;
  theme ??= 'dark';

  // Combined: use with state setters
  let debugMode = settings.debug;
  debugMode ??= false;

  // Type check: value is null, so ??= will assign
  let safeValue = value;
  safeValue ??= 42;

  return (
    <Box flexDirection="column" gap={1}>
      <Text bold>Logical Assignment Operators</Text>
      <Text></Text>
      <Text>||=: {displayName}</Text>
      <Text>&&=: {label}</Text>
      <Text>??=: theme={theme}</Text>
      <Text>??=: debug={String(debugMode)}</Text>
      <Text>??=: value={safeValue}</Text>
    </Box>
  );
}
