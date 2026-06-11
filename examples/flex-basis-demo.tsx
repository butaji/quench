// Flex Basis Demo — Quench
// Demonstrates flexBasis property for initial sizing
// flexBasis sets the initial size before flexGrow/shrink

import { render, Box, Text, useState, useInput, useApp } from 'ink';

function FlexBasisDemo(): JSX.Element {
  const [basis, setBasis] = useState<number>(10);
  const [grow, setGrow] = useState<number>(0);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === '+') setBasis(b => Math.min(b + 2, 40));
    if (input === '-') setBasis(b => Math.max(b - 2, 4));
    if (input === 'g') setGrow(g => (g + 1) % 4);
    if (input === 's') setGrow(0);
  });

  const boxes = [
    { label: 'Fixed 10', width: 10, color: 'cyan' as const },
    { label: `Basis ${basis}`, basis: basis, grow: grow, color: 'green' as const },
    { label: 'Grow 1', flexGrow: 1, color: 'yellow' as const },
  ];

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">flexBasis Demo</Text>
      <Text dimColor>[+/-] basis | [g] cycle grow | [s] shrink | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={2}>
        <Text dimColor>flexBasis:</Text>
        <Text color="cyan">{basis}</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>flexGrow:</Text>
        <Text color="yellow">{grow}</Text>
      </Box>

      <Text> </Text>
      
      {/* Row with varying sizing strategies */}
      <Box borderStyle="single" height={6} flexDirection="row" gap={1}>
        {/* Fixed width */}
        <Box backgroundColor="cyan" width={10} height={4} justifyContent="center" alignItems="center">
          <Text color="black">10px</Text>
        </Box>
        
        {/* Basis-based */}
        <Box backgroundColor="green" flexBasis={basis} flexGrow={grow} height={4} justifyContent="center" alignItems="center">
          <Text color="black">basis={basis}</Text>
        </Box>
        
        {/* Grows to fill remaining space */}
        <Box backgroundColor="yellow" flexGrow={1} height={4} justifyContent="center" alignItems="center">
          <Text color="black">grow=1</Text>
        </Box>
      </Box>

      <Text> </Text>
      <Box flexDirection="column">
        <Text dimColor small>Box 1: width=10 (fixed)</Text>
        <Text dimColor small>Box 2: flexBasis={basis} flexGrow={grow}</Text>
        <Text dimColor small>Box 3: flexGrow=1 (fills remainder)</Text>
      </Box>
    </Box>
  );
}

render(<FlexBasisDemo />);
