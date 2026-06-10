// Align Demo — TuiBridge
// Demonstrates alignSelf and alignContent properties for alignment
// Shows how children can override parent's alignItems (alignSelf)
// and how wrapped content is aligned (alignContent)

import { render, Box, Text, useState, useInput, useApp } from 'ink';

// alignContent options for wrapped multi-line content
const ALIGN_CONTENT_OPTIONS = ['flex-start', 'center', 'flex-end', 'stretch', 'space-between', 'space-around'] as const;

function AlignDemo(): JSX.Element {
  const [parentAlign, setParentAlign] = useState<'flex-start' | 'center' | 'flex-end'>('center');
  const [child1Align, setChild1Align] = useState<'auto' | 'flex-start' | 'center' | 'flex-end'>('auto');
  const [child2Align, setChild2Align] = useState<'auto' | 'flex-start' | 'center' | 'flex-end'>('auto');
  const [contentAlignIdx, setContentAlignIdx] = useState(0);
  const [wrap, setWrap] = useState(true);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'p') setParentAlign(a => a === 'flex-start' ? 'center' : a === 'center' ? 'flex-end' : 'flex-start');
    if (input === '1') setChild1Align(a => a === 'auto' ? 'flex-start' : a === 'flex-start' ? 'center' : a === 'center' ? 'flex-end' : 'auto');
    if (input === '2') setChild2Align(a => a === 'auto' ? 'flex-end' : a === 'flex-end' ? 'center' : a === 'center' ? 'flex-start' : 'auto');
    if (input === 'c') setContentAlignIdx(i => (i + 1) % ALIGN_CONTENT_OPTIONS.length);
    if (input === 'w') setWrap(w => !w);
  });

  const contentAlign = ALIGN_CONTENT_OPTIONS[contentAlignIdx];

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">alignSelf / alignContent Demo</Text>
      <Text dimColor>[p] parent align | [1/2] child alignSelf | [c] alignContent | [w] wrap | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={3}>
        <Text dimColor>alignItems:</Text>
        <Text color="cyan">{parentAlign}</Text>
      </Box>
      <Box flexDirection="row" gap={3}>
        <Text dimColor>alignSelf:</Text>
        <Text color="yellow">{child1Align === 'auto' ? '(auto)' : child1Align}, {child2Align === 'auto' ? '(auto)' : child2Align}</Text>
      </Box>
      <Box flexDirection="row" gap={3}>
        <Text dimColor>alignContent:</Text>
        <Text color="magenta">{contentAlign}</Text>
      </Box>
      <Box flexDirection="row" gap={3}>
        <Text dimColor>flexWrap:</Text>
        <Text color={wrap ? 'green' : 'gray'}>{wrap ? 'wrap' : 'nowrap'}</Text>
      </Box>

      <Text> </Text>
      
      {/* alignSelf demo - column layout */}
      <Box borderStyle="single" flexDirection="column" padding={1}>
        <Text dimColor small>alignSelf Demo (column)</Text>
        <Box 
          borderStyle="round"
          height={8}
          alignItems={parentAlign}
          flexDirection="column"
        >
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
      </Box>

      <Text> </Text>

      {/* alignContent demo - wrapped row layout */}
      <Box borderStyle="single" flexDirection="column" padding={1}>
        <Text dimColor small>alignContent Demo (wrapped row)</Text>
        <Box 
          borderStyle="round"
          width={35}
          height={10}
          flexDirection="row"
          flexWrap={wrap ? 'wrap' : 'nowrap'}
          alignContent={contentAlign}
          gap={1}
        >
          {Array.from({ length: 8 }).map((_, i) => (
            <Box
              key={i}
              width={10}
              height={3}
              backgroundColor={['red', 'green', 'blue', 'yellow', 'magenta', 'cyan', 'white', 'gray'][i % 8]}
              justifyContent="center"
              alignItems="center"
            >
              <Text color="black">{i + 1}</Text>
            </Box>
          ))}
        </Box>
      </Box>

      <Text> </Text>
      <Text dimColor small>
        alignSelf: override alignItems per child | alignContent: align wrapped lines
      </Text>
    </Box>
  );
}

render(<AlignDemo />);
