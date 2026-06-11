// useAnimation Demo — Quench
// Demonstrates useAnimation hook for smooth animations
// Shows frame counter, elapsed time, and delta timing

import { render, Box, Text, useState, useInput, useApp, useAnimation } from 'ink';

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const BOUNCE_FRAMES = ['●', '○', '◎', '○'];

function AnimationDemo(): JSX.Element {
  const [interval, setInterval] = useState(80);
  const [isActive, setIsActive] = useState(true);

  // Animation hook - shared timer, accurate frame/time/delta
  const { frame, time, delta, reset } = useAnimation({ interval, isActive });

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === ' ') setIsActive(a => !a);
    if (input === 'r') reset();
    if (input === '+') setInterval(i => Math.min(i + 20, 500));
    if (input === '-') setInterval(i => Math.max(i - 20, 20));
  });

  const spinnerChar = SPINNER_FRAMES[frame % SPINNER_FRAMES.length];
  const bounceChar = BOUNCE_FRAMES[frame % BOUNCE_FRAMES.length];

  // Sine wave animation
  const sineValue = Math.sin((time / 1000) * Math.PI * 2);
  const barWidth = Math.floor((sineValue + 1) * 15); // 0-30
  const bar = '█'.repeat(barWidth) + '░'.repeat(30 - barWidth);

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">useAnimation Demo</Text>
      <Text dimColor>[space] pause | [r] reset | [+/-] speed | [q] quit</Text>
      <Text> </Text>

      <Box flexDirection="row" gap={2}>
        <Text dimColor>Status:</Text>
        <Text color={isActive ? 'green' : 'red'}>{isActive ? 'running' : 'paused'}</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>Interval:</Text>
        <Text color="cyan">{interval}ms</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>Frame:</Text>
        <Text color="yellow">{frame}</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>Time:</Text>
        <Text color="magenta">{(time / 1000).toFixed(1)}s</Text>
      </Box>
      <Box flexDirection="row" gap={2}>
        <Text dimColor>Delta:</Text>
        <Text color="blue">{delta}ms</Text>
      </Box>

      <Text> </Text>

      {/* Spinner animation */}
      <Box flexDirection="row" gap={1}>
        <Text dimColor>Spinner:</Text>
        <Text bold color="green">{spinnerChar}</Text>
        <Text dimColor small>(frame % {SPINNER_FRAMES.length})</Text>
      </Box>

      {/* Bounce animation */}
      <Box flexDirection="row" gap={1}>
        <Text dimColor>Bounce:</Text>
        <Text bold color="cyan">{bounceChar}</Text>
        <Text dimColor small>(frame % {BOUNCE_FRAMES.length})</Text>
      </Box>

      {/* Sine wave */}
      <Box flexDirection="column" gap={1}>
        <Text dimColor>Sine wave (time-based):</Text>
        <Box width={30}>
          <Text color="yellow">{bar}</Text>
        </Box>
      </Box>

      <Text> </Text>
      <Text dimColor small>
        useAnimation shares a single timer internally for all components
      </Text>
    </Box>
  );
}

render(<AnimationDemo />);
