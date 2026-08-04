{
  if (globalThis.process && globalThis.process.stdin) {
    const stdin = globalThis.process.stdin;
    stdin.closed ??= false;
    stdin.errored ??= null;
    stdin.readableAborted ??= false;
    stdin.autoClose ??= false;
    stdin.bytesRead ??= 0;
  }
}
