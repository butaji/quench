// URI encoding example — exercises encodeURI, decodeURI,
// encodeURIComponent, decodeURIComponent.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (rquickjs)
//   3. runts build (codegen -> runts-ink)

import React from 'react';
import { Box, Text } from 'ink';

export default function UriEncodingDemo() {
  const text = 'hello world & foo=bar';
  const encoded = encodeURIComponent(text);
  const decoded = decodeURIComponent(encoded);
  const uri = 'https://example.com/path?query=hello world';
  const encodedUri = encodeURI(uri);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">URI Encoding Demo</Text>
      <Text></Text>
      <Text>Original: {text}</Text>
      <Text>Encoded: {encoded}</Text>
      <Text>Decoded: {decoded}</Text>
      <Text>URI: {encodedUri}</Text>
    </Box>
  );
}
