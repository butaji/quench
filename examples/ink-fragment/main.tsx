// Fragment example — demonstrates React fragment usage with Ink.
// Fragments allow grouping elements without adding extra nodes.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

render(<App />);
