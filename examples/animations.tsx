// Animations Demo — TuiBridge
// Demonstrates terminal animations using useEffect and intervals
// Real-world pattern for loading spinners, progress animations

import { render, Box, Text, useState, useEffect, useInput, useApp } from 'ink';

// Spinner animation
function Spinner({ delay = 80 }: { delay?: number }): JSX.Element {
  const [frame, setFrame] = useState(0);
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

  useEffect(() => {
    const interval = setInterval(() => {
      setFrame(f => (f + 1) % frames.length);
    }, delay);
    return () => clearInterval(interval);
  }, [delay]);

  return <Text color="cyan">{frames[frame]}</Text>;
}

// Progress animation
function AnimatedBar(): JSX.Element {
  const [progress, setProgress] = useState(0);

  useEffect(() => {
    const interval = setInterval(() => {
      setProgress(p => {
        if (p >= 100) {
          clearInterval(interval);
          return 100;
        }
        return p + 2;
      });
    }, 100);
    return () => clearInterval(interval);
  }, []);

  const width = 30;
  const filled = Math.round((progress / 100) * width);
  const bar = '█'.repeat(filled) + '░'.repeat(width - filled);

  return (
    <Text color="green">[{bar}] {progress}%</Text>
  );
}

// Blinking text
function BlinkingText({ children }: { children: string }): JSX.Element {
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    const interval = setInterval(() => {
      setVisible(v => !v);
    }, 500);
    return () => clearInterval(interval);
  }, []);

  return (
    <Text inverse={visible} dimColor={!visible}>
      {visible ? children : '         '}
    </Text>
  );
}

// Pulse animation
function PulsingText({ children }: { children: string }): JSX.Element {
  const [intensity, setIntensity] = useState(0);
  const colors = ['gray', 'white', 'cyan', 'brightCyan'];

  useEffect(() => {
    const interval = setInterval(() => {
      setIntensity(i => (i + 1) % colors.length);
    }, 200);
    return () => clearInterval(interval);
  }, []);

  return (
    <Text color={colors[intensity]} bold>
      {children}
    </Text>
  );
}

function AnimationsDemo(): JSX.Element {
  const [demo, setDemo] = useState(0);
  const demos = ['spinner', 'progress', 'blinking', 'pulse'];

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === 'n') setDemo(d => (d + 1) % demos.length);
    if (input === 'p') setDemo(d => (d - 1 + demos.length) % demos.length);
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Animations Demo</Text>
      <Text dimColor>[n/p] next/prev | [q] quit</Text>
      <Text> </Text>
      
      <Box borderStyle="single" padding={1} minHeight={5}>
        <Text dimColor>Current: </Text>
        <Text color="yellow" bold>{demos[demo]}</Text>
        <Text> </Text>
        
        {demos[demo] === 'spinner' && (
          <Box flexDirection="column" gap={1}>
            <Box flexDirection="row" gap={1}>
              <Text>Slow: </Text>
              <Spinner delay={150} />
            </Box>
            <Box flexDirection="row" gap={1}>
              <Text>Normal: </Text>
              <Spinner delay={80} />
            </Box>
            <Box flexDirection="row" gap={1}>
              <Text>Fast: </Text>
              <Spinner delay={40} />
            </Box>
          </Box>
        )}
        
        {demos[demo] === 'progress' && (
          <Box flexDirection="column" gap={1}>
            <AnimatedBar />
            <Text dimColor small>Progress resets on re-mount</Text>
          </Box>
        )}
        
        {demos[demo] === 'blinking' && (
          <Box flexDirection="column" gap={1}>
            <BlinkingText>⚠ WARNING: This is blinking!</BlinkingText>
            <Text dimColor small>Uses inverse for visibility toggle</Text>
          </Box>
        )}
        
        {demos[demo] === 'pulse' && (
          <Box flexDirection="column" gap={1}>
            <PulsingText>▶ PULSING TEXT ◀</PulsingText>
            <Text dimColor small>Cycles through gray → white → cyan → brightCyan</Text>
          </Box>
        )}
      </Box>
    </Box>
  );
}

render(<AnimationsDemo />);
