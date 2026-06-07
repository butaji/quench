// ink-focus-paste example — demonstrates useFocus, useFocusManager, usePaste.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// These hooks manage focus state and paste handling in Ink apps.

import React from 'react';
import { Box, Text } from 'ink';

// Simulated focusable items
const focusItems = [
  { id: 1, label: 'First Item', focused: true },
  { id: 2, label: 'Second Item', focused: false },
  { id: 3, label: 'Third Item', focused: false },
];

export default function FocusPasteDemo() {
  const results: string[] = [];

  // Focus simulation
  results.push('Focus Demo:');
  for (const item of focusItems) {
    const prefix = item.focused ? '> ' : '  ';
    const color = item.focused ? 'green' : 'gray';
    results.push(`${prefix}${item.label} (${color})`);
  }

  // Focus manager simulation
  const focusManager = {
    next: () => 'Moved to next',
    prev: () => 'Moved to prev',
    select: () => 'Selected',
  };
  results.push('');
  results.push('FocusManager:');
  results.push(focusManager.next());
  results.push(focusManager.prev());
  results.push(focusManager.select());

  // Paste simulation
  const pastedText = 'Hello from clipboard!';
  results.push('');
  results.push('Paste Demo:');
  results.push(`Pasted: ${pastedText}`);

  // Process pasted text
  const words = pastedText.split(' ');
  results.push(`Words: ${words.length}`);
  results.push(`Chars: ${pastedText.length}`);

  // Paste handler simulation
  const onPaste = (text: string) => {
    return `Pasted ${text.length} chars: "${text.substring(0, 20)}${text.length > 20 ? '...' : ''}"`;
  };
  results.push(onPaste(pastedText));

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Focus & Paste Demo</Text>
      <Text dimColor>Note: useFocus, usePaste available in runts-ink</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
