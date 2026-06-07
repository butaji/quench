// Box metrics example — demonstrates measureElement and useBoxMetrics hooks.
// Simplified for parity: static dimension display

import React, { useRef, useState } from 'react';
import { Box, Text, measureElement, useBoxMetrics } from 'ink';

export default function BoxMetrics() {
  const ref = useRef(null);
  const [dims, setDims] = useState({ width: 0, height: 0 });
  const { measureElement } = useBoxMetrics();

  // Measure the box when ref is available
  React.useEffect(() => {
    if (ref.current) {
      const d = measureElement(ref.current);
      setDims(d);
    }
  }, []);

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">Box Metrics Demo</Text>
      <Text>Dimensions: {dims.width}x{dims.height}</Text>
      <Box ref={ref} width={10} height={3} marginTop={1} borderStyle="round" justifyContent="center" alignItems="center">
        <Text>Box</Text>
      </Box>
    </Box>
  );
}
