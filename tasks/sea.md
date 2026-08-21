# node:sea (Bun red)

Required behavior: preserve Bun's red not-implemented posture. `isSea` may
exist as a shape-compatible value, but no fake single-executable implementation
may claim support. Bun's documented alternative is `bun build --compile`.
Verify `require('node:sea')` and `isSea` behavior without mutating upstream
fixtures. Lint rules: file <=500, functions <=40, complexity <=10.