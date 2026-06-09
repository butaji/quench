// Import attributes example — demonstrates ES2024 import { type: 'json' } syntax
import React from 'react';
import { Box, Text } from 'ink';

// This example demonstrates JSON module imports and ES2024 import attributes.
//
// ES2024 import attributes syntax:
//   import config from './data.json' with { type: 'json' };
//
// The 'with { type: "json" }' attribute tells the runtime that the
// imported module should be parsed as JSON. This is useful for:
// - Type safety when importing config files
// - Build-time validation
// - Tree-shaking unused imports
//
// Since import attributes syntax is not yet universally supported,
// this example demonstrates the concept using inline data that
// would be equivalent to what would be loaded with import attributes.

function App() {
  // Data equivalent to: import config from './data.json' with { type: 'json' }
  // The actual JSON file contains: { "name": "ImportApp", "version": "2.0.0", ... }
  const name = "ImportApp";
  const version = "2.0.0";

  return (
    <Box flexDirection="column" padding={1}>
      <Text bold>Import Attributes Demo</Text>
      <Text>Name: {name}</Text>
      <Text>Version: {version}</Text>
      <Box marginTop={1}>
        <Text dimColor>{"ES2024: import x from 'y' with { type: 'json' }"}</Text>
      </Box>
      <Text dimColor>Parsed by oxc, erased at codegen time.</Text>
    </Box>
  );
}

export default App;
