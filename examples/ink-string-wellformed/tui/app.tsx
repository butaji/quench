// ink-string-wellformed example — demonstrates String.isWellFormed() and toWellFormed()
//
// This example exercises the ES2024 String well-formedness methods:
// - String.prototype.isWellFormed() - checks if string is valid UTF-16
// - String.prototype.toWellFormed() - returns a well-formed UTF-16 string
//
// These methods help detect and fix strings with lone surrogates,
// which can cause issues in many APIs and operations.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// isWellFormed() - check if string is valid UTF-16
const validStr = 'Hello, World! 你好世界 🎉';
const wellFormedCheck = validStr.isWellFormed();

// String with lone high surrogate
const loneHighSurrogate = 'Hello\uD800';
const loneHighWellFormed = loneHighSurrogate.isWellFormed();

// String with lone low surrogate
const loneLowSurrogate = 'World\uDC00';
const loneLowWellFormed = loneLowSurrogate.isWellFormed();

// String with both lone surrogates (unpaired)
const unpairedSurrogates = '\uD800\uD800';
const unpairedWellFormed = unpairedSurrogates.isWellFormed();

// Empty string is always well-formed
const emptyStr = '';
const emptyWellFormed = emptyStr.isWellFormed();

// ASCII string is always well-formed
const asciiStr = 'ASCII text 123';
const asciiWellFormed = asciiStr.isWellFormed();

// toWellFormed() - convert to well-formed string
const invalidStr = 'Test\uD800Data';
const wellFormedResult = invalidStr.toWellFormed();

// Check after toWellFormed
const afterWellFormed = wellFormedResult.isWellFormed();

// Complex example with multiple surrogates
const mixedSurrogates = '😀\uD800😀\uDC00😀';
const mixedWellFormed = mixedSurrogates.isWellFormed();

// Using toWellFormed on already well-formed string
const alreadyWell = 'Already valid 🎊';
const stillWell = alreadyWell.toWellFormed();
const stillWellCheck = stillWell.isWellFormed();

// Length comparison before and after toWellFormed
const invalidLength = invalidStr.length;
const wellFormedLength = wellFormedResult.length;

// Comparing strings
const sameContent = 'Same\uD800';
const wellSame = sameContent.toWellFormed();
const areEqual = sameContent === wellSame;

export default function StringWellFormed() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">String.isWellFormed() and toWellFormed()</Text>
      <Text></Text>
      <Text>isWellFormed() checks:</Text>
      <Text>  validStr: {validStr} - wellFormed: {wellFormedCheck ? 'yes' : 'no'}</Text>
      <Text>  emptyStr: wellFormed: {emptyWellFormed ? 'yes' : 'no'}</Text>
      <Text>  asciiStr: wellFormed: {asciiWellFormed ? 'yes' : 'no'}</Text>
      <Text></Text>
      <Text>Lone surrogates:</Text>
      <Text>  loneHighSurrogate: {loneHighSurrogate} - wellFormed: {loneHighWellFormed ? 'yes' : 'no'}</Text>
      <Text>  loneLowSurrogate: {loneLowSurrogate} - wellFormed: {loneLowWellFormed ? 'yes' : 'no'}</Text>
      <Text>  unpairedSurrogates: wellFormed: {unpairedWellFormed ? 'yes' : 'no'}</Text>
      <Text>  mixedSurrogates: wellFormed: {mixedWellFormed ? 'yes' : 'no'}</Text>
      <Text></Text>
      <Text>toWellFormed() results:</Text>
      <Text>  invalidStr: {invalidStr} (len={invalidLength})</Text>
      <Text>  toWellFormed: {wellFormedResult} (len={wellFormedLength})</Text>
      <Text>  isWellFormed after: {afterWellFormed ? 'yes' : 'no'}</Text>
      <Text></Text>
      <Text>Preservation:</Text>
      <Text>  alreadyWell: {alreadyWell}</Text>
      <Text>  still well after: {stillWellCheck ? 'yes' : 'no'}</Text>
      <Text>  sameContent === wellSame: {areEqual ? 'true' : 'false'}</Text>
    </Box>
  );
}
