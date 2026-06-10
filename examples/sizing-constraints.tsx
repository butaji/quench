// Sizing Constraints Demo — TuiBridge
// Demonstrates min/max width/height, position=absolute, display=none

import { render, Box, Text, useState, useInput, useApp } from 'ink';

function SizingConstraints(): JSX.Element {
  const [showHidden, setShowHidden] = useState(true);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 't') setShowHidden(h => !h);
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Sizing Constraints Demo</Text>
      <Text dimColor>[t] toggle hidden | [q] quit</Text>
      <Text> </Text>
      {/* min/max width */}
      <Box flexDirection="row" gap={1}>
        <Box borderStyle="single" minWidth={15} padding={1}>
          <Text>minWidth=15</Text>
        </Box>
        <Box borderStyle="single" maxWidth={10} padding={1}>
          <Text>maxWidth=10 truncates</Text>
        </Box>
      </Box>
      <Text> </Text>
      {/* position absolute */}
      <Box borderStyle="single" height={6} position="relative">
        <Text>Relative parent</Text>
        <Box position="absolute" top={2} left={4} borderStyle="round" borderColor="yellow" paddingX={1}>
          <Text>absolute @ (4,2)</Text>
        </Box>
      </Box>
      <Text> </Text>
      {/* display none toggle */}
      <Box flexDirection="row" gap={1}>
        <Box borderStyle="single" padding={1} display={showHidden ? 'flex' : 'none'}>
          <Text color="green">Visible</Text>
        </Box>
        <Box borderStyle="single" padding={1}>
          <Text>Always visible</Text>
        </Box>
      </Box>
    </Box>
  );
}

render(<SizingConstraints />);
