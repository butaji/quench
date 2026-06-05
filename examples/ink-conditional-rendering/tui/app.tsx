// Conditional rendering example — demonstrates different ways
// to conditionally show/hide content.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

function ConditionalContent({ show }: { show: boolean }) {
  // Ternary operator
  return show ? (
    <Text color="green">This is conditionally shown</Text>
  ) : (
    <Text dimColor>Content hidden</Text>
  );
}

function LoginStatus({ isLoggedIn }: { isLoggedIn: boolean }) {
  // Logical AND operator
  if (isLoggedIn) {
    return <Text color="cyan">Welcome back!</Text>;
  }
  return <Text color="yellow">Please log in</Text>;
}

export default function ConditionalDemo() {
  // Static values for parity testing
  const showContent = true;
  const isLoggedIn = false;
  const userCount = 0;
  const error = null;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Conditional Rendering Demo</Text>
      <Text></Text>
      
      <Text bold>Ternary operator:</Text>
      <ConditionalContent show={showContent} />
      
      <Text></Text>
      <Text bold>if/else pattern:</Text>
      <LoginStatus isLoggedIn={isLoggedIn} />
      
      <Text></Text>
      <Text bold>Logical AND (&&):</Text>
      {userCount > 0 && (
        <Text>You have {userCount} messages</Text>
      )}
      {userCount === 0 && (
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
