// useBridge Demo — TuiBridge
// Demonstrates useBridge() for accessing Rust-propagated props
// Run with: tuibridge --prop theme=dark --prop user=admin examples/use-bridge.tsx

import { render, Box, Text, useBridge, useInput, useApp } from 'ink';

function UseBridgeDemo(): JSX.Element {
  const bridge = useBridge();
  const config = bridge.config;

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">useBridge() Demo</Text>
      <Text dimColor>Pass --prop KEY=VALUE to inject config | [q] quit</Text>
      <Text> </Text>
      <Box borderStyle="single" padding={1}>
        <Text bold>User Props:</Text>
        {Object.keys(config).length === 0 ? (
          <Text dimColor>(no --prop flags passed)</Text>
        ) : (
          Object.entries(config).map(([k, v]) => (
            <Text key={k}>{k}: {String(v)}</Text>
          ))
        )}
      </Box>
      <Text> </Text>
      <Box borderStyle="single" padding={1}>
        <Text bold>Platform:</Text>
        <Text>OS: {config.platform?.os}</Text>
        <Text>Arch: {config.platform?.arch}</Text>
      </Box>
      <Text> </Text>
      <Box borderStyle="single" padding={1}>
        <Text bold>Terminal:</Text>
        <Text>Color Support: {config.terminal?.colorSupport}-bit</Text>
        <Text>Mouse: {String(config.terminal?.hasMouse)}</Text>
        <Text>Unicode: {String(config.terminal?.hasUnicode)}</Text>
      </Box>
    </Box>
  );
}

render(<UseBridgeDemo />);
