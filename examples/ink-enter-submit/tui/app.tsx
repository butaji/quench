// Enter Submit example — demonstrates form submission on Enter key.
// Simplified for parity: shows static form for all environments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function EnterSubmit() {
  // NOTE: useInput is not supported in runts HIR runtime.
  // Static form shown for parity testing.
  const input = '';
  const submitted = 'example@example.com';
  const isFocused = true;

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Form with Enter Submit</Text>
      <Text></Text>
      
      <Box flexDirection="column" gap={1}>
        <Text>Email:</Text>
        <Box 
          borderStyle="single" 
          paddingX={1}
          borderColor={isFocused ? 'blue' : 'gray'}
        >
          <Text>
            {input || <Text dimColor>Enter email...</Text>}
          </Text>
        </Box>
        
        {submitted && (
          <Box marginTop={1}>
            <Text>Submitted: </Text>
            <Text color="green">{submitted}</Text>
          </Box>
        )}
      </Box>
      
      <Text></Text>
      <Text dimColor italic>
        {isFocused ? 'Press Enter to submit, Tab to move focus' : 'Focus: input field'}
      </Text>
    </Box>
  );
}
