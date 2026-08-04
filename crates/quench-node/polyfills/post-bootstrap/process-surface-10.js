{
  if (globalThis.process && globalThis.process.stdin) {
    const stdin = globalThis.process.stdin;
    stdin.pipe ||= (destination) => destination;
    stdin.unpipe ||= () => stdin;
    stdin.wrap ||= () => stdin;
  }
}
