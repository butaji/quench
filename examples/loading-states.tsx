// Loading States Demo — TuiBridge
// Demonstrates various loading indicators
// Common pattern for async operations

import { render, Box, Text, useState, useEffect, useApp, useInput } from 'ink';

const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const DOTS_FRAMES = ['   ', '.  ', '.. ', '...'];
const BLOCK_FRAMES = ['▖', '▘', '▝', '▗'];
const PROGRESS_STATES = ['Installing', 'Compiling', 'Linking', 'Optimizing', 'Bundling', 'Done!'];

function Spinner({ label }: { label: string }): JSX.Element {
  const [frame, setFrame] = useState(0);
  
  useEffect(() => {
    const interval = setInterval(() => {
      setFrame(f => (f + 1) % SPINNER_FRAMES.length);
    }, 100);
    return () => clearInterval(interval);
  }, []);

  return (
    <Box flexDirection="row" gap={1}>
      <Text color="cyan">{SPINNER_FRAMES[frame]}</Text>
      <Text>{label}</Text>
    </Box>
  );
}

function DotsLoader({ label }: { label: string }): JSX.Element {
  const [frame, setFrame] = useState(0);
  
  useEffect(() => {
    const interval = setInterval(() => {
      setFrame(f => (f + 1) % DOTS_FRAMES.length);
    }, 300);
    return () => clearInterval(interval);
  }, []);

  return (
    <Box flexDirection="row" gap={1}>
      <Text color="yellow">{DOTS_FRAMES[frame]}</Text>
      <Text>{label}</Text>
    </Box>
  );
}

function BlockLoader({ label }: { label: string }): JSX.Element {
  const [frame, setFrame] = useState(0);
  
  useEffect(() => {
    const interval = setInterval(() => {
      setFrame(f => (f + 1) % BLOCK_FRAMES.length);
    }, 150);
    return () => clearInterval(interval);
  }, []);

  return (
    <Box flexDirection="row" gap={1}>
      <Text color="magenta">{BLOCK_FRAMES[frame]}</Text>
      <Text>{label}</Text>
    </Box>
  );
}

function ProgressBar({ progress, label }: { progress: number; label: string }): JSX.Element {
  const filled = Math.round(progress * 20);
  const empty = 20 - filled;
  const bar = '█'.repeat(filled) + '░'.repeat(empty);
  
  return (
    <Box flexDirection="column">
      <Box flexDirection="row" justifyContent="space-between">
        <Text>{label}</Text>
        <Text color="cyan">{Math.round(progress * 100)}%</Text>
      </Box>
      <Text color="green">{bar}</Text>
    </Box>
  );
}

function SkeletonLoader({ lines }: { lines: number }): JSX.Element {
  const [pulse, setPulse] = useState(false);
  
  useEffect(() => {
    const interval = setInterval(() => {
      setPulse(p => !p);
    }, 500);
    return () => clearInterval(interval);
  }, []);

  const shade = pulse ? 'gray' : 'white';
  
  return (
    <Box flexDirection="column" gap={1}>
      {Array.from({ length: lines }).map((_, i) => (
        <Box key={i} flexDirection="row" gap={1}>
          <Text dimColor>{'█'.repeat(Math.random() * 10 + 5)}</Text>
          <Text dimColor>{'░'.repeat(Math.random() * 15 + 10)}</Text>
        </Box>
      ))}
    </Box>
  );
}

function LoadingStatesDemo(): JSX.Element {
  const [mode, setMode] = useState<'spinners' | 'progress' | 'skeleton'>('spinners');
  const [progress, setProgress] = useState(0);
  const [taskState, setTaskState] = useState(0);

  useEffect(() => {
    if (mode !== 'progress') return;
    
    const interval = setInterval(() => {
      setProgress(p => {
        if (p >= 1) {
          clearInterval(interval);
          return 1;
        }
        return p + 0.02;
      });
      setTaskState(s => Math.min(s + 1, PROGRESS_STATES.length - 1));
    }, 100);

    return () => clearInterval(interval);
  }, [mode]);

  useInput((input: string) => {
    if (input === 'q') useApp().exit();
    if (input === '1') setMode('spinners');
    if (input === '2') {
      setMode('progress');
      setProgress(0);
      setTaskState(0);
    }
    if (input === '3') setMode('skeleton');
    if (input === 'r' && mode === 'progress') {
      setProgress(0);
      setTaskState(0);
    }
  });

  return (
    <Box flexDirection="column" padding={1} borderStyle="round">
      <Text bold color="green">Loading States Demo</Text>
      <Text dimColor>[1] spinners | [2] progress | [3] skeleton | [q] quit</Text>
      <Text> </Text>

      {mode === 'spinners' && (
        <Box flexDirection="column" gap={2} borderStyle="single" padding={1}>
          <Spinner label="Processing..." />
          <DotsLoader label="Loading data" />
          <BlockLoader label="Compiling" />
        </Box>
      )}

      {mode === 'progress' && (
        <Box flexDirection="column" gap={2} borderStyle="single" padding={1}>
          <Text bold color="cyan">{PROGRESS_STATES[taskState]}</Text>
          <ProgressBar progress={progress} label="Task" />
          <Text dimColor small>[r] restart</Text>
        </Box>
      )}

      {mode === 'skeleton' && (
        <Box flexDirection="column" gap={2} borderStyle="single" padding={1}>
          <Text bold>Loading content...</Text>
          <SkeletonLoader lines={4} />
        </Box>
      )}
    </Box>
  );
}

render(<LoadingStatesDemo />);
