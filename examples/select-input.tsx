// Select Input — TuiBridge
// Common UI pattern: keyboard-navigable select/choice component
// Real-world pattern used in CLI tools

import { render, Box, Text, useState, useInput, useApp } from 'ink';

interface SelectOption {
  label: string;
  value: string;
}

const OPTIONS: SelectOption[] = [
  { label: 'Install dependencies', value: 'install' },
  { label: 'Run tests', value: 'test' },
  { label: 'Build project', value: 'build' },
  { label: 'Deploy to production', value: 'deploy' },
  { label: 'Exit', value: 'exit' },
];

function SelectInput({ options, onSelect }: { 
  options: SelectOption[]; 
  onSelect: (value: string) => void;
}): JSX.Element {
  const [selectedIndex, setSelectedIndex] = useState(0);

  useInput((input: string, key: { downArrow: boolean; upArrow: boolean; return: boolean }) => {
    if (key.downArrow || input === 'j') {
      setSelectedIndex(i => (i + 1) % options.length);
    }
    if (key.upArrow || input === 'k') {
      setSelectedIndex(i => (i - 1 + options.length) % options.length);
    }
    if (key.return || input === ' ') {
      onSelect(options[selectedIndex].value);
    }
  });

  return (
    <Box flexDirection="column" gap={1}>
      {options.map((option, index) => {
        const isSelected = index === selectedIndex;
        return (
          <Box key={option.value} flexDirection="row">
            <Text color={isSelected ? 'cyan' : 'gray'}>
              {isSelected ? '► ' : '  '}
            </Text>
            <Text 
              color={isSelected ? 'white' : 'gray'}
              bold={isSelected}
            >
              {option.label}
            </Text>
          </Box>
        );
      })}
    </Box>
  );
}

function SelectDemo(): JSX.Element {
  const [selected, setSelected] = useState<string | null>(null);

  const handleSelect = (value: string) => {
    if (value === 'exit') {
      useApp().exit();
    } else {
      setSelected(value);
    }
  };

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Select Input Demo</Text>
      <Text dimColor>[↑/↓ or k/j] navigate | [Enter] select | [q] quit</Text>
      <Text> </Text>
      <Box borderStyle="single" padding={1}>
        <SelectInput options={OPTIONS} onSelect={handleSelect} />
      </Box>
      {selected && (
        <>
          <Text> </Text>
          <Text>
            Selected: <Text color="yellow" bold>{selected}</Text>
          </Text>
        </>
      )}
    </Box>
  );
}

render(<SelectDemo />);
