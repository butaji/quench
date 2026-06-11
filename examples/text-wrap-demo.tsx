// Text Wrap Demo — Quench
// Demonstrates wrap property for text handling (Ink 6: textWrap, Ink 7: wrap)
// Shows wrap and truncate modes. Note: ratatui has limited wrap modes vs Ink.

import { render, Box, Text, useState, useInput, useApp } from 'ink';

// Ink 7 wrap modes and Quench support:
// ✅ "wrap" - Word wrap (full support)
// ✅ "truncate" - Truncate with ellipsis (full support)
// ⚠️ "end", "middle" - Fall back to "wrap" (partial)
// ⚠️ "truncate-end", "truncate-middle", "truncate-start" - Fall back to "truncate" (partial)
const WRAP_OPTIONS = ['wrap', 'truncate', 'end', 'middle', 'truncate-end', 'truncate-start'] as const;
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
      <Text bold color="green">wrap/textWrap Demo</Text>
      <Text dimColor>[w] wrap mode | [+/-] width | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={2}>
        <Text dimColor>mode:</Text>
        <Text color="cyan">{wrapMode}</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>width:</Text>
        <Text color="yellow">{width}</Text>
      </Box>

      <Text> </Text>
      
      <Box borderStyle="single" padding={1}>
        <Box width={width}>
          {/* Use wrap (Ink 7) with textWrap (Ink 6) fallback */}
          <Text wrap={wrapMode}>{SAMPLE_TEXT}</Text>
        </Box>
      </Box>

      <Text> </Text>
      <Text dimColor small>
        Modes: wrap ✅ | truncate ✅ | end/middle ⚠️ | truncate-* ⚠️
      </Text>
    </Box>
  );
}

render(<TextWrapDemo />);
