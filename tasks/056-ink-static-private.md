# Task 056: `ink-static-private` Example — Static Methods, Private Fields `#field`

**Priority:** P2-Medium  
**Phase:** 6 — Classes & OOP  
**Depends on:** 055

## Problem

Zero examples use static methods or private fields (`#field`).

## Example

```tsx
import { Box, Text } from 'ink';

class Config {
  static version = '1.0';
  #secret: string;

  constructor(secret: string) {
    this.#secret = secret;
  }

  static getVersion() {
    return Config.version;
  }

  getSecret() {
    return this.#secret;
  }
}

export default function App() {
  const cfg = new Config('my-secret');
  return (
    <Box flexDirection="column">
      <Text>Version: {Config.getVersion()}</Text>
      <Text>Secret: {cfg.getSecret()}</Text>
    </Box>
  );
}
```

## Work

- Static methods → Rust associated functions
- Static properties → Rust const/static
- Private fields → Rust private struct fields

**Requires Task 074 (private fields in HIR).**

## Acceptance Criteria

- [ ] Example exists, renders identically in deno and `runts dev`
- [ ] Static methods produce compilable Rust
- [ ] Private fields `#field` parse into HIR and produce compilable Rust
- [ ] `runts build --release` produces working binary with 100% output match
