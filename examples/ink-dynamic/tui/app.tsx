// Dynamic example — demonstrates state-driven rendering.
// Exercises `color`, `bold`, `dimColor` on Text with useState.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { useState } from 'react';
import { Box, Text } from 'ink';

type Status = 'ok' | 'warn' | 'fail';

export default function Dynamic() {
  const [status] = useState<Status>('ok');

  const statusColor: Record<Status, string> = {
    ok: 'green',
    warn: 'yellow',
    fail: 'red',
  };

  return (
    <Box flexDirection="column" borderStyle="single" paddingX={2} paddingY={1}>
      <Text bold color="cyan">Live Status</Text>
      <Text color={statusColor[status] as any}>{status.toUpperCase()}</Text>
      <Text dimColor>Press r to refresh, q to quit.</Text>
    </Box>
  );
}
