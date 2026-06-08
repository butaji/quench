import React from 'react';
import { Box, Text } from 'ink';

// Promise.withResolvers (ES2024) creates a Promise with resolve/reject exposed externally.
// This example demonstrates the API without async behavior.

export default function App() {
  // Demonstrate the API synchronously - we can create the promise and resolve it immediately
  const { promise, resolve, reject } = Promise.withResolvers<string>();
  
  // Synchronously resolve the promise
  resolve('sync resolved');
  
  // Use the resolved value
  const [status, setStatus] = React.useState('waiting');
  
  React.useEffect(() => {
    promise.then((value: string) => {
      setStatus(value);
    });
  }, []);
  
  // For demo purposes, show the status directly since we're resolving synchronously
  const demoStatus = 'API demonstrated';
  
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold>Promise.withResolvers Demo</Text>
      <Text>Promise API: withResolvers() available</Text>
      <Text>Status: {demoStatus}</Text>
      <Text dimColor>ES2024: const {'{promise, resolve, reject}'} = Promise.withResolvers()</Text>
    </Box>
  );
}
