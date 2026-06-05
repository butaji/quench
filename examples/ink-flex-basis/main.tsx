// Flex basis example — demonstrates flexBasis property for Box.
// Shows how elements can have fixed or percentage-based sizes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

render(<App />);
