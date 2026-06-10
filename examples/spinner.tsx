// Spinner Example - TuiBridge demo (TypeScript)
// Demonstrates timer-driven animation and color cycling

import { render, Box, Text, useState, useEffect } from 'ink';

function Spinner(): JSX.Element {
  const [frame, setFrame] = useState(0);
  const [colorIndex, setColorIndex] = useState(0);
  
  const spinnerFrames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  const colors = ['cyan', 'magenta', 'yellow', 'green', 'blue'];
  
  useEffect(() => {
    const timer = setInterval(() => {
      setFrame((f: number) => (f + 1) % spinnerFrames.length);
      setColorIndex((c: number) => (c + 1) % colors.length);
    }, 100);
    
    return () => clearInterval(timer);
  }, []);
  
  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Spinner Demo</Text>
      <Box marginTop={1} justifyContent="center">
        <Text color={colors[colorIndex]} bold>{spinnerFrames[frame]}</Text>
        <Text> Loading...</Text>
      </Box>
      <Box marginTop={1} justifyContent="center">
        <Text dimColor>Frame: {frame}/{spinnerFrames.length - 1}</Text>
      </Box>
      <Box marginTop={1} justifyContent="center">
        <Text dimColor>Color: {colors[colorIndex]}</Text>
      </Box>
    </Box>
  );
}

render(<Spinner />);
