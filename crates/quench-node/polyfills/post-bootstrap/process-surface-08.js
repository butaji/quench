{
  if (globalThis.process && globalThis.process.stdin) {
    const stdin = globalThis.process.stdin;
    stdin.fd ??= 0;
    stdin.destroyed ??= false;
    stdin.readableEncoding ??= null;
  }
}
