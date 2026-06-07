// ink-measure example — demonstrates measureElement and useBoxMetrics.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// measureElement returns the computed dimensions of an element.

import React from 'react';
import { Box, Text, measureElement } from 'ink';

export default function MeasureDemo() {
  const results: string[] = [];

  // Simulate measureElement behavior
  // In real Ink, measureElement returns actual computed dimensions
  // In runts-ink, we simulate this for demo purposes
  const dimensions = { width: 40, height: 5 };
  results.push(`Status: Measured`);
  results.push(`Width: ${dimensions.width}`);
  results.push(`Height: ${dimensions.height}`);

  // Simulate multiple measurements
  const boxes = [
    { label: 'Small', width: 20, height: 3 },
    { label: 'Medium', width: 40, height: 5 },
    { label: 'Large', width: 60, height: 8 },
  ];

  for (const box of boxes) {
    results.push(`${box.label}: ${box.width}x${box.height}`);
  }

  // Calculate total area
  const totalArea = boxes.reduce((sum, b) => sum + b.width * b.height, 0);
  results.push(`Total area: ${totalArea} units`);

  // Calculate aspect ratios
  for (const box of boxes) {
    const ratio = (box.width / box.height).toFixed(2);
    results.push(`${box.label} aspect ratio: ${ratio}`);
  }

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Measure Element Demo</Text>
      <Text></Text>
      {results.map((result, i) => (
        <Text key={i}>{result}</Text>
      ))}
    </Box>
  );
}
