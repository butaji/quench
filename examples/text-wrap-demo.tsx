// Text Wrap Demo — TuiBridge
// Demonstrates textWrap property for text handling
// Shows wrap, truncate, and other truncation modes

import { render, Box, Text, useState, useInput, useApp } from 'ink';

const WRAP_OPTIONS = ['wrap', 'end', 'middle', 'truncate-end', 'truncate', 'truncate-middle', 'truncate-start'] as const;
type WrapOption = typeof WRAP_OPTIONS[number];

const SAMPLE_TEXT = "The quick brown fox jumps over the lazy dog. This is a longer text to demonstrate wrapping behavior.";

function TextWrapDemo(): JSX.Element {
  const [wrapIdx, setWrapIdx] = useState(0);
  const [width, setWidth] = useState(30);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'w') setWrapIdx(i => (i + 1) % WRAP_OPTIONS.length);
    if (input === '+') setWidth(w => Math.min(w + 5, 60));
    if (input === '-') setWidth(w => Math.max(w - 5, 15));
  });

  const wrapMode = WRAP_OPTIONS[wrapIdx];

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">textWrap Demo</Text>
      <Text dimColor>[w] wrap mode | [+/-] width | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={2}>
        <Text dimColor>wrap:</Text>
        <Text color="cyan">{wrapMode}</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>width:</Text>
        <Text color="yellow">{width}</Text>
      </Box>

      <Text> </Text>
      
      <Box borderStyle="single" padding={1}>
        <Box width={width}>
          <Text wrap={wrapMode}>{SAMPLE_TEXT}</Text>
        </Box>
      </Box>

      <Text> </Text>
      <Text dimColor small>
        Mode explanations:
      </Text>
      <Text dimColor small>
        wrap - Wrap to multiple lines
      </Text>
      <Text dimColor small>
        end - Wrap at end of line
      </Text>
      <Text dimColor small>
        truncate-end - Truncate at end with ellipsis
      </Text>
      <Text dimColor small>
        truncate-start - Truncate at start
      </Text>
    </Box>
  );
}

render(<TextWrapDemo />);
