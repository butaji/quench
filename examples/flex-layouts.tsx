// Flex Layouts — TuiBridge
// Demonstrates all Flexbox alignment and distribution options
// Based on Ink's yoga-based layout system

import { render, Box, Text, useState, useInput, useApp } from 'ink';

const ALIGN_OPTIONS = ['flex-start', 'center', 'flex-end', 'stretch', 'baseline'] as const;
const JUSTIFY_OPTIONS = ['flex-start', 'center', 'flex-end', 'space-between', 'space-around', 'space-evenly'] as const;
const ALIGN_CONTENT_OPTIONS = ['flex-start', 'center', 'flex-end', 'stretch', 'space-between', 'space-around'] as const;

function FlexDemo(): JSX.Element {
  const [alignIdx, setAlignIdx] = useState(0);
  const [justifyIdx, setJustifyIdx] = useState(0);
  const [contentIdx, setContentIdx] = useState(0);
  const [wrap, setWrap] = useState(false);
  const [gap, setGap] = useState(0);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'a') setAlignIdx(i => (i + 1) % ALIGN_OPTIONS.length);
    if (input === 'j') setJustifyIdx(i => (i + 1) % JUSTIFY_OPTIONS.length);
    if (input === 'c') setContentIdx(i => (i + 1) % ALIGN_CONTENT_OPTIONS.length);
    if (input === 'w') setWrap(w => !w);
    if (input === 'g') setGap(g => (g + 1) % 4);
  });

  const align = ALIGN_OPTIONS[alignIdx];
  const justify = JUSTIFY_OPTIONS[justifyIdx];
  const contentAlign = ALIGN_CONTENT_OPTIONS[contentIdx];

  const boxes = [
    { color: 'red', height: 3 },
    { color: 'green', height: 5 },
    { color: 'blue', height: 4 },
    { color: 'yellow', height: 6 },
  ];

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Flex Layouts Demo</Text>
      <Text dimColor>[a] alignItems | [j] justifyContent | [c] alignContent | [w] wrap | [g] gap | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={1}>
        <Text dimColor width={14}>alignItems:</Text>
        <Text color="cyan">{align}</Text>
      </Box>
      <Box flexDirection="row" gap={1}>
        <Text dimColor width={14}>justifyContent:</Text>
        <Text color="green">{justify}</Text>
      </Box>
      <Box flexDirection="row" gap={1}>
        <Text dimColor width={14}>alignContent:</Text>
        <Text color="magenta">{contentAlign}</Text>
      </Box>
      <Box flexDirection="row" gap={1}>
        <Text dimColor width={14}>flexWrap:</Text>
        <Text color={wrap ? 'yellow' : 'gray'}>{wrap ? 'wrap' : 'nowrap'}</Text>
      </Box>
      <Box flexDirection="row" gap={1}>
        <Text dimColor width={14}>gap:</Text>
        <Text color="cyan">{gap}</Text>
      </Box>

      <Text> </Text>
      
      {/* Demo area */}
      <Box 
        borderStyle="single" 
        height={12}
        alignItems={align}
        justifyContent={justify}
        flexWrap={wrap ? 'wrap' : 'nowrap'}
        alignContent={wrap ? contentAlign : undefined}
        gap={gap}
      >
        {boxes.map((box, i) => (
          <Box 
            key={i}
            width={10}
            height={box.height}
            backgroundColor={box.color}
            justifyContent="center"
            alignItems="center"
          >
            <Text color="black">{box.height}</Text>
          </Box>
        ))}
      </Box>

      <Text> </Text>
      <Text dimColor small>
        alignItems: child cross-axis | justifyContent: main-axis | alignContent: wrapped lines
      </Text>
    </Box>
  );
}

render(<FlexDemo />);
