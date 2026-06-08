// ink-index-intersection example — demonstrates index signatures and intersection types.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Index signatures and intersection types are erased at compile time.
// They have no runtime impact on the generated JavaScript or Rust code.

import React from 'react';
import { Box, Text } from 'ink';

// --- Base interface ---
interface BaseProps {
  name: string;
}

// --- Style interface with index signature ---
interface StyleProps {
  color: string;
  width: number;
  // Index signature allows any string key with string | number value
  [key: string]: string | number;
}

// --- Event interface with index signature ---
interface EventProps {
  onClick?: () => void;
  onHover?: () => void;
  // Numeric index signature
  [index: number]: string;
}

// --- Intersection type: Base & Style ---
type FullProps = BaseProps & StyleProps;

// --- Nested intersection types ---
interface A {
  a: string;
}
interface B {
  b: number;
}
interface C {
  c: boolean;
}
type ABC = A & B & C;

// --- Object with index signature ---
interface Dictionary {
  [word: string]: string;
}

// --- Numeric index signature ---
interface StringArray {
  [index: number]: string;
  length: number;
}

// --- Combined: required props + index signature ---
interface FlexibleWidget {
  id: string;
  label: string;
  [key: string]: string | number | boolean;
}

// --- Actual objects ---
const props: FullProps = {
  name: 'Widget',
  color: 'blue',
  width: 80,
  padding: 8, // Extra prop via index signature
};

const abc: ABC = {
  a: 'hello',
  b: 42,
  c: true,
};

const dict: Dictionary = {
  hello: 'world',
  foo: 'bar',
  baz: 'qux',
};

const strArr: StringArray = ['first', 'second', 'third'];

const widget: FlexibleWidget = {
  id: 'w1',
  label: 'My Widget',
  size: 100,
  active: true,
};

// --- Function with index signature return type ---
function createMeta(prefix: string): { [key: string]: string } {
  return {
    [`${prefix}_name`]: 'value',
    [`${prefix}_id`]: '123',
  };
}

export default function IndexIntersectionDemo() {
  const results: string[] = [];

  // FullProps (intersection)
  results.push(`Name: ${props.name}`);
  results.push(`Color: ${props.color}`);
  results.push(`Width: ${props.width}`);
  results.push(`Padding (index): ${props.padding}`);

  // ABC (intersection chain)
  results.push(`ABC.a: ${abc.a}`);
  results.push(`ABC.b: ${abc.b}`);
  results.push(`ABC.c: ${abc.c}`);

  // Dictionary (index signature)
  results.push(`Dict entries:`);
  for (const [key, value] of Object.entries(dict)) {
    results.push(`  ${key} = ${value}`);
  }

  // StringArray (numeric index)
  results.push(`StringArray[0]: ${strArr[0]}`);
  results.push(`StringArray.length: ${strArr.length}`);

  // FlexibleWidget (required + index)
  results.push(`Widget.id: ${widget.id}`);
  results.push(`Widget.label: ${widget.label}`);
  results.push(`Widget.size (index): ${widget.size}`);
  results.push(`Widget.active (index): ${widget.active}`);

  // Dynamic index signature
  const meta = createMeta('meta');
  results.push(`Meta entries:`);
  for (const [key, value] of Object.entries(meta)) {
    results.push(`  ${key} = ${value}`);
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Index Signatures & Intersection Types Demo</Text>
      <Text dimColor>All type-level features erased at compile time</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
