// Measure Ref Demo — Quench
// Demonstrates useRef + measureElement for responsive layout

import { render, Box, Text, useState, useRef, useEffect, useInput, useApp, measureElement } from 'ink';

interface BoxMetrics {
  width: number;
  height: number;
}

function MeasureRefDemo(): JSX.Element {
  const boxRef = useRef<{ id?: number }>({});
  const [dims, setDims] = useState<BoxMetrics>({ width: 0, height: 0 });

  useEffect(() => {
    const timer = setInterval(() => {
      const m = measureElement(boxRef);
      if (m) setDims(m);
    }, 500);
    return () => clearInterval(timer);
  }, []);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">useRef + measureElement Demo</Text>
      <Text dimColor>Resize terminal to see dimensions update | [q] quit</Text>
      <Text> </Text>
      <Box ref={boxRef} borderStyle="single" padding={2} flexGrow={1}>
        <Text bold>Tracked Box</Text>
        <Text>Width: {dims.width.toFixed(1)} cols</Text>
        <Text>Height: {dims.height.toFixed(1)} rows</Text>
      </Box>
      <Text> </Text>
      <Text dimColor>This box uses flexGrow=1 to fill available space.</Text>
      <Text dimColor>measureElement reads Yoga-computed layout after each commit.</Text>
    </Box>
  );
}

render(<MeasureRefDemo />);
