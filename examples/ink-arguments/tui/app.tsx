// ink-arguments example — demonstrates the legacy `arguments` object.
//
// The `arguments` object is available in non-arrow functions and contains
// all parameters passed to the function. While rest parameters (`...args`)
// are preferred in modern code, `arguments` is still encountered in older
// JavaScript/TypeScript codebases.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust

import React from 'react';
import { Box, Text } from 'ink';

export default function ArgumentsDemo() {
  // Functions using the arguments object
  // Note: arguments is only available in non-arrow functions
  function sumAll(): number {
    let sum = 0;
    for (let i = 0; i < arguments.length; i++) {
      sum += arguments[i];
    }
    return sum;
  }

  function logArgs(): string {
    let result = '';
    for (let i = 0; i < arguments.length; i++) {
      if (i > 0) result += ', ';
      result += String(arguments[i]);
    }
    return result;
  }

  function maxOf(): number {
    if (arguments.length === 0) return 0;
    let max = arguments[0];
    for (let i = 1; i < arguments.length; i++) {
      if (arguments[i] > max) max = arguments[i];
    }
    return max;
  }

  function toArray(): string[] {
    const arr: string[] = [];
    for (let i = 0; i < arguments.length; i++) {
      arr.push(String(arguments[i]));
    }
    return arr;
  }

  const sum1 = sumAll(1, 2, 3);
  const sum2 = sumAll(10, 20, 30, 40, 50);
  const sum3 = sumAll();

  const args1 = logArgs('a', 'b', 'c');
  const args2 = logArgs('hello', 'world');
  const args3 = logArgs('only one');

  const max1 = maxOf(3, 1, 4, 1, 5, 9, 2, 6);
  const max2 = maxOf(42);
  const max3 = maxOf();

  const arr1 = toArray('x', 'y', 'z');
  const arr2 = toArray('single');

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Arguments Object Demo</Text>
      <Text></Text>
      <Text>--- sumAll() ---</Text>
      <Text>sumAll(1, 2, 3): {sum1}</Text>
      <Text>sumAll(10, 20, 30, 40, 50): {sum2}</Text>
      <Text>sumAll(): {sum3}</Text>
      <Text></Text>
      <Text>--- logArgs() ---</Text>
      <Text>logArgs('a', 'b', 'c'): {args1}</Text>
      <Text>logArgs('hello', 'world'): {args2}</Text>
      <Text>logArgs('only one'): {args3}</Text>
      <Text></Text>
      <Text>--- maxOf() ---</Text>
      <Text>maxOf(3, 1, 4, 1, 5, 9, 2, 6): {max1}</Text>
      <Text>maxOf(42): {max2}</Text>
      <Text>maxOf(): {max3}</Text>
      <Text></Text>
      <Text>--- toArray() ---</Text>
      <Text>toArray('x', 'y', 'z'): {arr1.join(', ')}</Text>
      <Text>toArray('single'): {arr2.join(', ')}</Text>
    </Box>
  );
}
