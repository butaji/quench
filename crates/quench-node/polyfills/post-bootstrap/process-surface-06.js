{
  if (globalThis.process && globalThis.process.stdin) {
    globalThis.process.stdin.isPaused ||= () => false;
  }
}
