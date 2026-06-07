# Task 066: `ink-animation` Example — `useAnimation`, `measureElement`, `useBoxMetrics`

**Priority:** P2-Medium
**Phase:** 6 — Ink Advanced
**Depends on:** 065

## Problem

`useAnimation`, `measureElement`, and `useBoxMetrics` are implemented in `js_bridge.rs` but have zero example coverage.

## Example

```tsx
import { Box, Text, useAnimation } from 'ink';

export default function App() {
  const { frame, isPlaying } = useAnimation({ fps: 10, maxFrames: 10 });
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  const spinner = frames[frame % frames.length];

  return (
    <Box flexDirection="column">
      <Text>{spinner} Frame {frame}</Text>
      <Text>{isPlaying ? 'Playing' : 'Stopped'}</Text>
    </Box>
  );
}
```

For parity, use `--once` to capture only the initial frame.

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] `useAnimation` exercised
- [ ] `runts build --release` produces working binary
- [ ] Parity harness 100%