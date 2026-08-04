{
  if (globalThis.process && globalThis.process.stdin) {
    const stdin = globalThis.process.stdin;
    stdin.destroy ||= () => stdin;
    stdin.ref ||= () => stdin;
    stdin.unref ||= () => stdin;
  }
}
