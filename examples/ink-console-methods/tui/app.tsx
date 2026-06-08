// Console methods example — exercises console.log, error, warn, info, time, timeEnd, table.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen->runts-ink)

import React, { useEffect } from 'react';
import { Box, Text } from 'ink';

export default function ConsoleMethodsDemo() {
  useEffect(() => {
    console.time('render');
    console.log('App mounted');
    console.info('Info message');
    console.warn('Warning message');
    console.error('Error message');
    console.timeEnd('render');
  }, []);

  const data = [
    { name: 'Alice', age: 30 },
    { name: 'Bob', age: 25 },
  ];
  console.table(data);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Console Methods Demo</Text>
      <Text></Text>
      <Text>Exercised:</Text>
      <Text>  - console.log</Text>
      <Text>  - console.info</Text>
      <Text>  - console.warn</Text>
      <Text>  - console.error</Text>
      <Text>  - console.time / timeEnd</Text>
      <Text>  - console.table</Text>
      <Text></Text>
      <Text dimColor>Press q to quit.</Text>
    </Box>
  );
}
