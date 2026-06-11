// Focus Manager Demo — Quench
// Demonstrates useFocus + useFocusManager for keyboard navigation

import { render, Box, Text, useState, useInput, useApp, useFocus, useFocusManager } from 'ink';

function FocusableItem({ label, id }: { label: string; id: string }) {
  const { isFocused } = useFocus({ id });
  return (
    <Box
      borderStyle={isFocused ? 'round' : 'single'}
      borderColor={isFocused ? 'yellow' : 'gray'}
      paddingX={2}
      paddingY={1}
      backgroundColor={isFocused ? '#333' : undefined}
    >
      <Text color={isFocused ? 'yellow' : 'white'}>{isFocused ? '► ' : '  '}{label}</Text>
    </Box>
  );
}

function FocusManagerDemo(): JSX.Element {
  const [items] = useState(['Dashboard', 'Settings', 'Profile', 'Logout']);
  const { focusNext, focusPrevious } = useFocusManager();

  useInput((input: string, key: { downArrow: boolean; upArrow: boolean; return: boolean }) => {
    if (input === 'q') useApp().exit();
    if (key.downArrow || input === 'j') focusNext();
    if (key.upArrow || input === 'k') focusPrevious();
    if (key.return) {
      // Enter pressed on focused item
    }
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Focus Manager Demo</Text>
      <Text dimColor>[j/↓] next | [k/↑] prev | [q] quit</Text>
      <Text> </Text>
      <Box flexDirection="column" gap={1}>
        {items.map(label => <FocusableItem key={label} label={label} id={label} />)}
      </Box>
    </Box>
  );
}

render(<FocusManagerDemo />);
