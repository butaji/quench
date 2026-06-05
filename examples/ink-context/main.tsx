// Context example — demonstrates React Context with Ink.
// Shows how to use context to pass data through the component tree.
//
// All three environments must produce the same look:
//   1. deno (real Ink)
//   2. runts dev (HIR runtime)
//   3. runts build (codegen->runts-ink)

import React, { createContext, useContext } from 'react';
import { render } from 'ink';
import App from './tui/app.tsx';

render(<App />);
