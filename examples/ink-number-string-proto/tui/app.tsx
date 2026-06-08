// ink-number-string-proto example — demonstrates Number and String prototype methods
//
// This example exercises:
// - Number.prototype.toFixed, toPrecision, toExponential
// - Number.isNaN, Number.isFinite, Number.isInteger
// - String.prototype.slice, substring, substr
// - String.prototype.indexOf, lastIndexOf, includes, startsWith, endsWith
// - String.prototype.toUpperCase, toLowerCase, trim, padStart, padEnd
// - String.prototype.replace, replaceAll, split, repeat
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

// Number prototype methods
const pi = 3.14159;
const fixed2 = pi.toFixed(2);
const precision4 = pi.toPrecision(4);
const exponential = pi.toExponential(2);

const num = 123.456;
const isNaN = Number.isNaN(NaN);
const notNaN = Number.isNaN(num);
const isFinite = Number.isFinite(42);
const notFinite = Number.isFinite(Infinity);
const isInt = Number.isInteger(42);
const notInt = Number.isInteger(3.14);

// String prototype methods
const str = 'Hello, World!';

// Slice, substring, substr
const slice1 = str.slice(0, 5);
const slice2 = str.slice(-6);
const subStr = str.substring(0, 5);
const subStr2 = str.substring(7, 12);

// Index methods
const indexOf = str.indexOf('World');
const lastIndex = 'hello world hello'.lastIndexOf('hello');
const includes = str.includes('World');
const startsWith = str.startsWith('Hello');
const endsWith = str.endsWith('!');

// Case methods
const upper = str.toUpperCase();
const lower = str.toLowerCase();
const trimmed = '  hello  '.trim();

// Padding
const padded = '42'.padStart(5, '0');
const paddedEnd = '42'.padEnd(5, '.');
const paddedStr = 'Hi'.padStart(10);

// Replace and split
const replaced = str.replace('World', 'TypeScript');
const split = str.split(' ');
const repeated = 'ha'.repeat(3);

// String search
const match = str.match(/[A-Z][a-z]+/g);

// String from char codes
const fromCharCode = String.fromCharCode(72, 101, 108, 108, 111);

// Char at and char code at
const charAt3 = str.charAt(3);
const charCodeAt0 = str.charCodeAt(0);

// Unicode
const emoji = '😀';
const emojiLength = emoji.length;
const emojiCodePoint = emoji.codePointAt(0);

export default function NumberStringProtoDemo() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Number and String Prototype Methods</Text>
      <Text></Text>
      <Text>Number methods:</Text>
      <Text>  pi.toFixed(2) = {fixed2}</Text>
      <Text>  pi.toPrecision(4) = {precision4}</Text>
      <Text>  pi.toExponential(2) = {exponential}</Text>
      <Text></Text>
      <Text>Number static methods:</Text>
      <Text>  isNaN(NaN) = {isNaN ? 'true' : 'false'}</Text>
      <Text>  isNaN(123.456) = {notNaN ? 'true' : 'false'}</Text>
      <Text>  isFinite(42) = {isFinite ? 'true' : 'false'}</Text>
      <Text>  isFinite(Infinity) = {notFinite ? 'true' : 'false'}</Text>
      <Text>  isInteger(42) = {isInt ? 'true' : 'false'}</Text>
      <Text>  isInteger(3.14) = {notInt ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>String slicing:</Text>
      <Text>  slice(0,5) = {slice1}</Text>
      <Text>  slice(-6) = {slice2}</Text>
      <Text>  substring(0,5) = {subStr}</Text>
      <Text>  substring(7,12) = {subStr2}</Text>
      <Text></Text>
      <Text>String search:</Text>
      <Text>  indexOf(World) = {indexOf}</Text>
      <Text>  lastIndexOf(hello) = {lastIndex}</Text>
      <Text>  includes(World) = {includes ? 'true' : 'false'}</Text>
      <Text>  startsWith(Hello) = {startsWith ? 'true' : 'false'}</Text>
      <Text>  endsWith(!) = {endsWith ? 'true' : 'false'}</Text>
      <Text></Text>
      <Text>String case/trim:</Text>
      <Text>  toUpperCase() = {upper}</Text>
      <Text>  toLowerCase() = {lower}</Text>
      <Text>  trim() = {trimmed}</Text>
      <Text></Text>
      <Text>String padding:</Text>
      <Text>  padStart(5, 0) = {padded}</Text>
      <Text>  padEnd(5, .) = {paddedEnd}</Text>
      <Text>  padStart(10) = {paddedStr}</Text>
      <Text></Text>
      <Text>String replace/split/repeat:</Text>
      <Text>  replace(World, TS) = {replaced}</Text>
      <Text>  split( ) = [{split.join(', ')}]</Text>
      <Text>  repeat(3) = {repeated}</Text>
      <Text></Text>
      <Text>Char methods:</Text>
      <Text>  fromCharCode(72,101,108,108,111) = {fromCharCode}</Text>
      <Text>  charAt(3) = {charAt3}</Text>
      <Text>  charCodeAt(0) = {charCodeAt0}</Text>
      <Text></Text>
      <Text>Unicode:</Text>
      <Text>  emoji length: {emojiLength}</Text>
      <Text>  emoji codePoint: {emojiCodePoint}</Text>
    </Box>
  );
}
