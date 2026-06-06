# Task: Add Missing Ink Examples for Full Feature Coverage

**Priority:** P1-High  
**Phase:** 1 — rquickjs + Yoga Engine  
**Depends on:** 024, 025

## Problem

89 examples exist but several Ink features are **not exercised by any example**. This means the js_bridge.rs supports them but they are never verified in parity tests.

| Feature | Example Needed | Why |
|---------|---------------|-----|
| `useAnimation` | `ink-animation` | Timer-based re-rendering, not covered |
| `usePaste` | `ink-paste` | Bracketed paste event handling |
| `measureElement` / `useBoxMetrics` | `ink-measure` | Layout readback for dynamic positioning |
| `useRef` | `ink-ref` | Mutable refs outside React state |
| `minWidth` / `maxWidth` | `ink-advanced-layout` | Constraint-based sizing |
| `zIndex` | `ink-z-index` | Layer ordering |
| `alignSelf` | `ink-align-self` | Per-child alignment override |
| `flexShrink` | `ink-flex-shrink` | Compression behavior |
| `columnGap` / `rowGap` | `ink-gaps` | Already exists but minimal |

## Rules for New Examples

- **Only `.ts`/`.tsx`** — no Rust code.
- Must have `tui/app.tsx` exporting a default component.
- Must have `main.tsx` that imports from `ink` and calls `render(<App />)`.
- Must have `deno.json` with `npm:ink@^5` import.
- Must be runnable with `deno run -A main.tsx`.

## Steps

1. Create `examples/ink-animation/tui/app.tsx`:
   ```tsx
   import React, { useState, useEffect } from 'react';
   import { Box, Text } from 'ink';

   export default function Animation() {
     const [frame, setFrame] = useState(0);
     useEffect(() => {
       const id = setInterval(() => setFrame(f => f + 1), 200);
       return () => clearInterval(id);
     }, []);
     const bars = ['▁','▃','▅','▆','▇','█','▇','▆','▅','▃'];
     return <Text>{bars[frame % bars.length]}</Text>;
   }
   ```

2. Create `examples/ink-paste/tui/app.tsx`:
   ```tsx
   import React, { useState } from 'react';
   import { Box, Text, useInput, usePaste } from 'ink';

   export default function PasteDemo() {
     const [pasted, setPasted] = useState('');
     usePaste({ active: true }, (text) => setPasted(text));
     useInput((input) => { if (input === 'q') process.exit(0); });
     return <Text>Pasted: {pasted || '<nothing>'}</Text>;
   }
   ```

3. Create `examples/ink-measure/tui/app.tsx`:
   ```tsx
   import React, { useRef, useEffect, useState } from 'react';
   import { Box, Text, measureElement } from 'ink';

   export default function MeasureDemo() {
     const ref = useRef();
     const [size, setSize] = useState({ width: 0, height: 0 });
     useEffect(() => {
       const s = measureElement(ref);
       setSize(s);
     }, []);
     return (
       <Box ref={ref} width={20} height={5} borderStyle="single">
         <Text>{size.width}x{size.height}</Text>
       </Box>
     );
   }
   ```

4. Create `examples/ink-advanced-layout/tui/app.tsx`:
   ```tsx
   import React from 'react';
   import { Box, Text } from 'ink';

   export default function AdvancedLayout() {
     return (
       <Box flexDirection="row" width={40}>
         <Box minWidth={10} flexShrink={1}><Text>A</Text></Box>
         <Box minWidth={10} flexShrink={2}><Text>B</Text></Box>
         <Box width={15} zIndex={2}><Text>C</Text></Box>
       </Box>
     );
   }
   ```

5. Add `deno.json` and `main.tsx` to each.

## Acceptance Criteria

- [ ] 4 new example directories created.
- [ ] Each example runs with `deno run -A main.tsx` without error.
- [ ] Each example renders something visible and deterministic.
- [ ] `runts dev --once --plugin ratatui <example>` produces identical output to deno.
- [ ] Examples are added to any example-list documentation.
