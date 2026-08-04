{
  if (globalThis.process && globalThis.process.stdin) {
    const stdin = globalThis.process.stdin;
    stdin[Symbol.asyncDispose] ||= async () => undefined;
  }
}
