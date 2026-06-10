// Spacing Props Demo — TuiBridge
// Demonstrates individual margin/padding sides + X/Y variants

import { render, Box, Text, useState, useInput, useApp } from 'ink';

function SpacingProps(): JSX.Element {
  const [mode, setMode] = useState(0);
  const modes = ['margin', 'padding'] as const;
  const current = modes[mode];

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'm') setMode(0);
    if (input === 'p') setMode(1);
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Spacing Props Demo</Text>
      <Text dimColor>[m] margin | [p] padding | [q] quit</Text>
      <Text>Showing: {current}</Text>
      <Text> </Text>
      <Box flexDirection="column" gap={1}>
        {current === 'margin' ? (
          <>
            <Box borderStyle="single" marginTop={2} padding={1}><Text>marginTop=2</Text></Box>
            <Box borderStyle="single" marginBottom={2} padding={1}><Text>marginBottom=2</Text></Box>
            <Box borderStyle="single" marginLeft={4} padding={1}><Text>marginLeft=4</Text></Box>
            <Box borderStyle="single" marginRight={4} padding={1}><Text>marginRight=4</Text></Box>
            <Box flexDirection="row">
              <Box borderStyle="single" marginX={2} padding={1}><Text>marginX=2</Text></Box>
            </Box>
            <Box borderStyle="single" marginY={2} padding={1}><Text>marginY=2</Text></Box>
          </>
        ) : (
          <>
            <Box borderStyle="single" paddingTop={2}><Text>paddingTop=2</Text></Box>
            <Box borderStyle="single" paddingBottom={2}><Text>paddingBottom=2</Text></Box>
            <Box borderStyle="single" paddingLeft={4}><Text>paddingLeft=4</Text></Box>
            <Box borderStyle="single" paddingRight={4}><Text>paddingRight=4</Text></Box>
            <Box borderStyle="single" paddingX={2}><Text>paddingX=2</Text></Box>
            <Box borderStyle="single" paddingY={2}><Text>paddingY=2</Text></Box>
          </>
        )}
      </Box>
    </Box>
  );
}

render(<SpacingProps />);
