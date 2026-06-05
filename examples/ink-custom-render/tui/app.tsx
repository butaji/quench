// Custom render example — demonstrates custom components and fragments.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text, Spacer } from 'ink';

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Box flexDirection="column" borderStyle="single" padding={1} marginY={1}>
      <Text bold color="cyan">{title}</Text>
      <Text></Text>
      {children}
    </Box>
  );
}

function Metric({ label, value }: { label: string; value: string | number }) {
  return (
    <Box flexDirection="row" gap={1}>
      <Text dimColor>{label}:</Text>
      <Text bold>{value}</Text>
    </Box>
  );
}

export default function CustomRenderExample() {
  const [count] = useState(42);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Custom Render Demo</Text>
      <Spacer />
      <Section title="Metrics">
        <Metric label="Dynamic count" value={count} />
        <Metric label="Status" value="ready" />
      </Section>
      <Spacer />
      <Text italic dimColor>
        Custom components compose like regular Ink components.
      </Text>
    </Box>
  );
}
