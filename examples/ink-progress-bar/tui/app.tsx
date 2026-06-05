// Progress bar example — demonstrates useAnimation for animated UI.
// Shows a progress bar that fills up over time.
//
// 1. deno: deno run -A main.tsx
// 2. runts dev: runts dev examples/ink-progress-bar
// 3. runts compile: runts build examples/ink-progress-bar --plugin ratatui --release

import React, { useEffect, useState } from 'react';
import { Box, Text, useInput, useApp } from 'ink';

function ProgressBar({ progress }: { progress: number }) {
  const width = 30;
  const filled = Math.floor((progress / 100) * width);
  const empty = width - filled;
  
  return (
    <Box>
      <Text>[</Text>
      <Text backgroundColor="green" color="black">
        {'█'.repeat(filled)}
      </Text>
      <Text dimColor>
        {'░'.repeat(empty)}
      </Text>
      <Text>]</Text>
      <Text> {Math.round(progress)}%</Text>
    </Box>
  );
}

export default function ProgressBarExample() {
  const [progress, setProgress] = useState(0);
  const { exit } = useApp();

  useInput((input, key) => {
    if (input === 'q' || key.escape) {
      exit();
    }
  });

  useEffect(() => {
    const interval = setInterval(() => {
      setProgress((prev) => {
        if (prev >= 100) {
          clearInterval(interval);
          return 100;
        }
        return prev + 2;
      });
    }, 100);
    
    return () => clearInterval(interval);
  }, []);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Progress Bar Demo</Text>
      <Text></Text>
      <Text>Animated progress using useEffect and state:</Text>
      <Text></Text>
      <Box padding={1} borderStyle="round" borderColor="green">
        <ProgressBar progress={progress} />
      </Box>
      <Text></Text>
      <Text dimColor>Press q or esc to quit.</Text>
    </Box>
  );
}
