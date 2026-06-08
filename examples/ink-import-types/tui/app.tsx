// ink-import-types example — demonstrates `import("...").Type` syntax.
//
// `import("./types.js").TypeName` is a TypeScript feature for importing
// types from other modules without importing runtime values.
//
// All three environments must produce the same look:
//   1. deno (real Ink) — reference implementation
//   2. runts dev (rquickjs) — TSX->JS transpile
//   3. runts build (compile path) — codegen->Rust
//
// NOTE: Type-level constructs are erased at compile time. The import type
// syntax becomes nothing in the generated JavaScript.

import React from 'react';
import { Box, Text } from 'ink';

// --- Import types using the import() syntax ---
type UserType = import('../types.ts').User;
type ProductType = import('../types.ts').Product;
type IDType = import('../types.ts').ID;
type StatusType = import('../types.ts').Status;
type ConfigType = import('../types.ts').Config;

// --- Use the imported types ---
const user: UserType = { name: 'Alice', age: 30 };
const product: ProductType = { id: 'prod-123', price: 99.99 };
const userId: IDType = 'user-456';
const productId: IDType = 789;
const status: StatusType = 'active';
const config: ConfigType = { debug: true, maxItems: 100 };

export default function App() {
  return (
    <Box flexDirection="column" padding={1}>
      <Text bold color="cyan">import() Type Syntax Demo</Text>
      <Text dimColor>type T = import("./module").Type</Text>
      <Text></Text>

      <Text>User (import type):</Text>
      <Text>  name: {user.name}</Text>
      <Text>  age: {user.age}</Text>

      <Text></Text>
      <Text>Product (import type):</Text>
      <Text>  id: {product.id}</Text>
      <Text>  price: ${product.price.toFixed(2)}</Text>

      <Text></Text>
      <Text>IDs (import type - union of string | number):</Text>
      <Text>  userId: {userId}</Text>
      <Text>  productId: {productId}</Text>

      <Text></Text>
      <Text>Status (import type - string literal):</Text>
      <Text>  current: {status}</Text>

      <Text></Text>
      <Text>Config (import type - interface):</Text>
      <Text>  debug: {String(config.debug)}</Text>
      <Text>  maxItems: {config.maxItems}</Text>
    </Box>
  );
}
