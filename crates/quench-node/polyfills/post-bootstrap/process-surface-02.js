{
  if (globalThis.process && globalThis.process.stdin) {
    globalThis.process.stdin.readableHighWaterMark ??= 65536;
  }
}
