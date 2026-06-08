// ArrayBuffer, Uint8Array, and DataView example
// Exercises binary data APIs available in JavaScript runtimes.
// 
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (compile path)

import React from 'react';
import { Box, Text } from 'ink';

export default function ArrayBufferDemo() {
  // Create an 8-byte buffer
  const buffer = new ArrayBuffer(8);
  
  // View it as unsigned 8-bit integers
  const uint8 = new Uint8Array(buffer);
  uint8[0] = 72; // 'H'
  uint8[1] = 105; // 'i'
  
  // View the same buffer as 64-bit floats via DataView
  const dataView = new DataView(buffer);
  dataView.setFloat64(0, 3.14159265358979, true); // little-endian
  
  // Read back as hex
  const hex = Array.from(uint8).map(b => b.toString(16).padStart(2, '0')).join(' ');
  
  // Read back as float
  const pi = dataView.getFloat64(0, true);
  
  return (
    <Box flexDirection="column" paddingX={2} paddingY={1} borderStyle="single">
      <Text bold>ArrayBuffer / Uint8Array / DataView</Text>
      <Text>Binary data handling in TUI</Text>
      <Text color="cyan">Buffer[8 bytes]: {hex}</Text>
      <Text color="green">Pi from DataView: {pi.toFixed(5)}</Text>
      <Text dimColor>Uint8Array: {8} bytes allocated</Text>
    </Box>
  );
}
