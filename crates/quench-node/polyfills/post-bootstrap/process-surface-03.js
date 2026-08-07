{
  if (globalThis.process && globalThis.process.stdin) {
    globalThis.process.stdin.readableLength ??= 0;
  }
}
