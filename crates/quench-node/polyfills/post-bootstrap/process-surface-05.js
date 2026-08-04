{
  if (globalThis.process && globalThis.process.stdin) {
    globalThis.process.stdin.read ||= () => null;
    globalThis.process.stdin.unshift ||= () => globalThis.process.stdin;
  }
}
