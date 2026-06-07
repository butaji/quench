// ink-animation example — demonstrates useAnimation hook for frame-based animation.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: In the compile path, animation requires Rust Future support.

import React from 'react';
import { Box, Text, useAnimation } from 'ink';

// Simple spinner animation frames
const SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

// Progress bar animation
const PROGRESS_FRAMES = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '▊'];

export default function AnimationDemo() {
  // useAnimation hook provides frame counter and playback state
  // Note: useAnimation is available in runts-ink but not in real Ink
  // For demo purposes, we'll simulate the animation behavior

  const results: string[] = [];

  // Simulate animation frame access
  const currentFrame = 0;
  const maxFrames = 10;

  // Spinner frame
  const spinnerFrame = currentFrame % SPINNER_FRAMES.length;
  results.push(`Spinner: ${SPINNER_FRAMES[spinnerFrame]} Frame ${currentFrame}/${maxFrames}`);

  // Progress bar
  const progressFrame = currentFrame % PROGRESS_FRAMES.length;
  const progress = Math.floor((currentFrame / maxFrames) * 100);
  results.push(`Progress: [${PROGRESS_FRAMES[progressFrame]}] ${progress}%`);

  // Animation state
  const isPlaying = true;
  const isPaused = false;
  results.push(`State: ${isPlaying ? 'Playing' : 'Stopped'}, ${isPaused ? 'Paused' : 'Running'}`);

  // Frame rate info
  const fps = 10;
  const duration = maxFrames / fps;
  results.push(`Animation: ${fps} FPS, ${duration}s duration`);

  // Simulate different animation types
  const animations = [
    { name: 'spinner', frame: 0, total: 10 },
    { name: 'progress', frame: 0, total: 100 },
    { name: 'bounce', frame: 0, total: 4 },
  ];

  for (const anim of animations) {
    const pct = Math.floor((anim.frame / anim.total) * 100);
    results.push(`${anim.name}: frame ${anim.frame}/${anim.total} (${pct}%)`);
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Animation Demo</Text>
      <Text dimColor>Note: useAnimation available in runts-ink</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
