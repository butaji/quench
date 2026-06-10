// Border Styles Demo — TuiBridge
// Demonstrates borderColor, borderDimColor, individual sides, title

import { render, Box, Text, useState, useInput, useApp } from 'ink';

const STYLES = ['single', 'double', 'round', 'bold'] as const;

function BorderStyles(): JSX.Element {
  const [idx, setIdx] = useState(0);
  const [dim, setDim] = useState(false);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'j') setIdx(i => (i + 1) % STYLES.length);
    if (input === 'k') setIdx(i => (i - 1 + STYLES.length) % STYLES.length);
    if (input === 'd') setDim(d => !d);
  });

  const style = STYLES[idx];

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Border Styles Demo</Text>
      <Text dimColor>[j/k] style | [d] dim | [q] quit</Text>
      <Text> </Text>
      {/* Full border with color */}
      <Box borderStyle={style} borderColor="cyan" borderDimColor={dim} padding={1} title={`Style: ${style}`}>
        <Text>Full border with color</Text>
      </Box>
      <Text> </Text>
      {/* Individual sides */}
      <Box flexDirection="row" gap={1}>
        <Box borderStyle="single" borderTop={false} padding={1}>
          <Text>no top</Text>
        </Box>
        <Box borderStyle="single" borderBottom={false} padding={1}>
          <Text>no bottom</Text>
        </Box>
        <Box borderStyle="single" borderLeft={false} padding={1}>
          <Text>no left</Text>
        </Box>
        <Box borderStyle="single" borderRight={false} padding={1}>
          <Text>no right</Text>
        </Box>
      </Box>
      <Text> </Text>
      {/* Color + dim indicator */}
      <Text dimColor>Dim: {String(dim)} | Style: {style}</Text>
    </Box>
  );
}

render(<BorderStyles />);