// Re-render example — demonstrates component updates.
// NOTE: useState is not fully supported in runts HIR runtime.
// Static values shown for parity testing.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime) - shows static
//   3. runts build (codegen->runts-ink) - full interactivity

import React from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

render(<App />);
