// Multi-Select Demo — TuiBridge
// Demonstrates checkbox-style selection with multiple items
// Common pattern for package selection, filter toggles

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface SelectItem {
  id: string;
  label: string;
  selected: boolean;
  description?: string;
}

const INITIAL_ITEMS: SelectItem[] = [
  { id: 'rust', label: 'Rust', selected: false, description: 'Systems programming language' },
  { id: 'typescript', label: 'TypeScript', selected: true, description: 'Typed JavaScript' },
  { id: 'go', label: 'Go', selected: false, description: 'Simple, reliable, efficient' },
  { id: 'python', label: 'Python', selected: true, description: 'Batteries included' },
  { id: 'zig', label: 'Zig', selected: false, description: 'Low-level, no hidden control flow' },
  { id: 'nim', label: 'Nim', selected: false, description: 'Efficient, expressive, elegant' },
];

function Checkbox({ checked, children }: { checked: boolean; children: React.ReactNode }): JSX.Element {
  return (
    <Text>
      {checked ? <Text color="green">[x]</Text> : <Text color="gray">[ ]</Text>}
      {' '}{children}
    </Text>
  );
}

function MultiSelectDemo(): JSX.Element {
  const [items, setItems] = useState(INITIAL_ITEMS);
  const [focusIdx, setFocusIdx] = useState(0);

  useInput((input: string, key) => {
    if (input === 'q') useApp().exit();
    
    if (key.upArrow) {
      setFocusIdx(i => Math.max(0, i - 1));
    }
    if (key.downArrow) {
      setFocusIdx(i => Math.min(items.length - 1, i + 1));
    }
    if (input === ' ' || input === 'x') {
      setItems(items.map((item, i) => 
        i === focusIdx ? { ...item, selected: !item.selected } : item
      ));
    }
    if (input === 'a') {
      // Select all
      setItems(items.map(item => ({ ...item, selected: true })));
    }
    if (input === 'n') {
      // Select none
      setItems(items.map(item => ({ ...item, selected: false })));
    }
  });

  const selectedCount = items.filter(i => i.selected).length;

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Multi-Select Demo</Text>
      <Text dimColor>[↑/↓] move | [Space/x] toggle | [a] all | [n] none | [q] quit</Text>
      <Text> </Text>

      <Box borderStyle="single" padding={1}>
        {items.map((item, idx) => {
          const isFocused = idx === focusIdx;
          return (
            <Box 
              key={item.id}
              flexDirection="column"
              marginBottom={1}
            >
              <Box flexDirection="row" gap={1}>
                <Text>
                  {isFocused && <Text color="cyan">► </Text>}
                  {!isFocused && <Text>  </Text>}
                </Text>
                <Checkbox checked={item.selected}>
                  <Text bold={isFocused} color={isFocused ? 'cyan' : 'white'}>
                    {item.label}
                  </Text>
                </Checkbox>
              </Box>
              {isFocused && item.description && (
                <Box marginLeft={4}>
                  <Text dimColor small>{item.description}</Text>
                </Box>
              )}
            </Box>
          );
        })}
      </Box>

      <Text> </Text>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>Selected:</Text>
        <Text color="green">{selectedCount}</Text>
        <Text dimColor>/</Text>
        <Text>{items.length}</Text>
      </Box>

      <Text> </Text>
      <Text dimColor small>
        {items.filter(i => i.selected).map(i => i.label).join(', ') || 'None'}
      </Text>
    </Box>
  );
}

render(<MultiSelectDemo />);
