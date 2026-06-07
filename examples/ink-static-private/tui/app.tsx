// ink-static-private — demonstrates static methods, static properties, and private fields.
//
// All three environments must produce the same output:
//   1. deno (real Ink) — npm:ink
//   2. runts dev (rquickjs) — TSX transpiled to JS
//   3. runts build (compile path) — TSX → HIR → Rust codegen
//
import React from 'react';
import { Box, Text } from 'ink';

class Config {
  static version = '1.0';
  static defaultTimeout = 30;
  #secret: string;

  constructor(secret: string) {
    this.#secret = secret;
  }

  static getVersion() {
    return Config.version;
  }

  static getDefaults() {
    return {
      version: Config.version,
      timeout: Config.defaultTimeout,
    };
  }

  getSecret() {
    return this.#secret;
  }

  #getInternal() {
    return 'internal';
  }

  reveal() {
    return this.#getInternal();
  }
}

export default function App() {
  const cfg = new Config('my-secret');
  const defaults = Config.getDefaults();

  return (
    <Box flexDirection="column">
      <Text>Version: {Config.getVersion()}</Text>
      <Text>Defaults: v{defaults.version}, {defaults.timeout}s</Text>
      <Text>Secret: {cfg.getSecret()}</Text>
      <Text>Internal: {cfg.reveal()}</Text>
    </Box>
  );
}
