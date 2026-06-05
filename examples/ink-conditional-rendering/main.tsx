// Conditional rendering example — demonstrates different ways
// to conditionally show/hide content.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

render(<App />);
