// Align Self Demo — TuiBridge
// Demonstrates alignSelf property for child alignment
// Shows how individual children can override parent's alignItems

import { render, Box, Text, useState, useInput, useApp } from 'ink';

function AlignDemo(): JSX.Element {
  const [parentAlign, setParentAlign] = useState<'flex-start' | 'center' | 'flex-end'>('center');
  const [child1Align, setChild1Align] = useState<'auto' | 'flex-start' | 'center' | 'flex-end'>('auto');
  const [child2Align, setChild2Align] = useState<'auto' | 'flex-start' | 'center' | 'flex-end'>('auto');

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'p') setParentAlign(a => a === 'flex-start' ? 'center' : a === 'center' ? 'flex-end' : 'flex-start');
    if (input === '1') setChild1Align(a => a === 'auto' ? 'flex-start' : a === 'flex-start' ? 'center' : a === 'center' ? 'flex-end' : 'auto');
    if (input === '2') setChild2Align(a => a === 'auto' ? 'flex-end' : a === 'flex-end' ? 'center' : a === 'center' ? 'flex-start' : 'auto');
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">alignSelf Demo</Text>
      <Text dimColor>[p] parent align | [1/2] child alignSelf | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={3}>
        <Text dimColor>Parent:</Text>
        <Text color="cyan">{parentAlign}</Text>
      </Box>
      <Box flexDirection="row" gap={3}>
        <Text dimColor>Child 1:</Text>
        <Text color={child1Align === 'auto' ? 'gray' : 'yellow'}>{child1Align}</Text>
      </Box>
      <Box flexDirection="row" gap={3}>
        <Text dimColor>Child 2:</Text>
        <Text color={child2Align === 'auto' ? 'gray' : 'yellow'}>{child2Align}</Text>
      </Box>

      <Text> </Text>
      
      {/* Container with configurable alignItems */}
      <Box 
        borderStyle="single" 
        height={8}
        alignItems={parentAlign}
        flexDirection="column"
      >
        {/* Each child can override with alignSelf */}
        <Box 
          backgroundColor="red" 
          width={8} 
          height={3}
          alignSelf={child1Align === 'auto' ? undefined : child1Align}
          justifyContent="center"
        >
          <Text color="white">A</Text>
        </Box>
        <Box 
          backgroundColor="green" 
          width={8} 
          height={3}
          alignSelf={child2Align === 'auto' ? undefined : child2Align}
          justifyContent="center"
        >
          <Text color="white">B</Text>
        </Box>
      </Box>

      <Text> </Text>
      <Text dimColor small>
        alignSelf overrides parent's alignItems for individual children
      </Text>
    </Box>
  );
}

render(<AlignDemo />);
