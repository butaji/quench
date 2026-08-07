{
  if (globalThis.process && globalThis.process.stdin) {
    globalThis.process.stdin.end ??= null;
  }
}
