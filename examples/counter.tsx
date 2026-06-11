// Counter Example - Quench demo (TypeScript)
// Demonstrates useState, useEffect, useInput with the Ink API

import { render, Box, Text, useState, useInput, useApp, useEffect } from 'ink';

interface CounterProps {}

function Counter(_props: CounterProps): JSX.Element {
  const [count, setCount] = useState(0);

  useInput((input: string) => {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === ' ') {
      setCount((c: number) => c + 1);
    }
  });

  useEffect(() => {
    const timer = setInterval(() => setCount((c: number) => c + 1), 1000);
    return () => clearInterval(timer);
  }, []);

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text color="green" bold>Counter App</Text>
      <Text>Count: {count}</Text>
      <Text dimColor>[space] increment | [q] quit</Text>
    </Box>
  );
}

render(<Counter />);
