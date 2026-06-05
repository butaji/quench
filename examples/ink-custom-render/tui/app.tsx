// Custom render example — demonstrates render options and custom components.
// Shows how to use Static for performance and how render options work.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-custom-render
// 3. runts compile: runts build examples/ink-custom-render --plugin ratatui --release

import React from 'react';
import { Box, Text, Static, Newline } from 'ink';

// A component that renders static content (not re-rendered on state changes)
function StaticHeader() {
  return (
    <>
      <Text bold color="cyan">Custom Render Demo</Text>
      <Newline />
      <Text dimColor>Static content is rendered once for performance.</Text>
      <Newline />
    </>
  );
}

// Dynamic content that changes
function DynamicContent({ count }: { count: number }) {
  return (
    <Box flexDirection="column">
      <Text>Dynamic count: {count}</Text>
      <Text dimColor>This content updates with state.</Text>
    </Box>
  );
}

export default function CustomRenderExample() {
  return (
    <Box flexDirection="column" padding={1}>
      <StaticHeader />
      <Newline />
      <DynamicContent count={42} />
      <Newline />
      <Text italic dimColor>
        Render options control stdout, exit, and debug modes.
      </Text>
    </Box>
  );
}
