{
  if (globalThis.process && globalThis.process.stdin) {
    const stdin = globalThis.process.stdin;
    stdin.close ||= () => stdin;
    stdin.pending ??= false;
  }
}
