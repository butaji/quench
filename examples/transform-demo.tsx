// Transform Component Demo — Quench
// Demonstrates text transformation using ANSI codes
// Transform applies transformations to text output

import { render, Box, Text, useState, useInput, useApp, useMemo } from 'ink';

// Simulate Transform functionality with custom rendering
// Transform takes children and applies a transform function

function TransformText({ text, transform }: { text: string; transform: (s: string) => string }): JSX.Element {
  const transformed = useMemo(() => transform(text), [text, transform]);
  return <Text>{transformed}</Text>;
}

// Simpler text transformation utilities
function reverseText(s: string): string {
  return s.split('').reverse().join('');
}

function boldify(s: string): string {
  return `\x1b[1m${s}\x1b[0m`;
}

function dimify(s: string): string {
  return `\x1b[2m${s}\x1b[0m`;
}

function underlify(s: string): string {
  return `\x1b[4m${s}\x1b[0m`;
}

function blinkify(s: string): string {
  return `\x1b[5m${s}\x1b[0m`;
}

function TransformDemo(): JSX.Element {
  const [text, setText] = useState('Hello Quench');
  const [transformMode, setTransformMode] = useState<'normal' | 'reverse' | 'bold' | 'dim' | 'underline' | 'blink'>('normal');

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === '1') setTransformMode('normal');
    if (input === '2') setTransformMode('reverse');
    if (input === '3') setTransformMode('bold');
    if (input === '4') setTransformMode('dim');
    if (input === '5') setTransformMode('underline');
    if (input === '6') setTransformMode('blink');
    if (input === ' ') {
      setText(t => t.length > 1 ? t.slice(1) : 'Hello Quench');
    }
  });

  const transform = (s: string): string => {
    switch (transformMode) {
      case 'reverse': return reverseText(s);
      case 'bold': return boldify(s);
      case 'dim': return dimify(s);
      case 'underline': return underlify(s);
      case 'blink': return blinkify(s);
      default: return s;
    }
  };

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Transform Demo (ANSI)</Text>
      <Text dimColor>[1-6] transform | [space] shorten | [q] quit</Text>
      <Text> </Text>

      <Box borderStyle="single" padding={1}>
        <TransformText text={text} transform={transform} />
      </Box>

      <Text> </Text>
      <Text dimColor>Transform: {transformMode}</Text>
    </Box>
  );
}

render(<TransformDemo />);
