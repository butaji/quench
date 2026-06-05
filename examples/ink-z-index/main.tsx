// Z-index example — demonstrates stacking order with absolute positioning.
// Shows how elements can overlap using position: absolute with z-index.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

render(<App />);
