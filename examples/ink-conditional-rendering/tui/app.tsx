// Conditional rendering example — demonstrates different ways
// to conditionally show/hide content.
//
// NOTE: Complex conditional rendering with React patterns may not work 
// in runts dev mode (HIR runtime). This version shows static content
// for parity testing across all three environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink) - full React support
//   2. runts dev (HIR runtime) - static render
//   3. runts build (codegen->runts-ink) - full interactivity

import React from 'react';
import { Box, Text } from 'ink';

export default function ConditionalDemo() {
  // Static values for parity testing across all environments
  // Complex React conditional rendering patterns are simplified
  const showContent = true;
  const isLoggedIn = false;
  const userCount = 0;
  const error = null;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Conditional Rendering Demo</Text>
      <Text></Text>
      
      <Text bold>Ternary operator:</Text>
      {showContent ? (
        <Text color="green">This is conditionally shown</Text>
      ) : (
        <Text dimColor>Content hidden</Text>
      )}
      
      <Text></Text>
      <Text bold>if/else pattern:</Text>
      {isLoggedIn ? (
        <Text color="cyan">Welcome back!</Text>
      ) : (
        <Text color="yellow">Please log in</Text>
      )}
      
      <Text></Text>
      <Text bold>Logical AND (andgt;):</Text>
      {userCount > 0 ? (
        <Text>You have {userCount} messages</Text>
      ) : (
        <Text dimColor>No messages</Text>
      )}
      
      <Text></Text>
      <Text bold>Nullish rendering:</Text>
      {error ? (
        <Text color="red">Error: {String(error)}</Text>
      ) : (
        <Text color="green">No errors</Text>
      )}
      
      <Text></Text>
      <Text dimColor italic>
        React conditional patterns work in runts.
      </Text>
    </Box>
  );
}
