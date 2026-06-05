// Fragment example — demonstrates React fragment usage with Ink.
// Fragments allow grouping elements without adding extra nodes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { Fragment } from 'react';
import { Box, Text } from 'ink';

function FragmentDemo() {
  const items = ['Alpha', 'Beta', 'Gamma'];

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Fragment Demo</Text>
      <Text></Text>
      
      <Text>Using fragments to group elements:</Text>
      <Text></Text>
      
      {/* Fragment groups items without adding a Box */}
      <Box>
        {items.map((item, i) => (
          <Fragment key={item}>
            <Text color="green">{item}</Text>
            {i < items.length - 1 && <Text dimColor> | </Text>}
          </Fragment>
        ))}
      </Box>
      
      <Text></Text>
      <Text dimColor>Fragments help reduce nesting depth.</Text>
    </Box>
  );
}

export default FragmentDemo;
