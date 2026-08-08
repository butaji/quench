{
  if (globalThis.process && globalThis.process.stdout) {
    const stdout = globalThis.process.stdout;
    stdout.writableHighWaterMark = 65536;
    if (stdout.constructor.name !== "Socket") {
      Object.defineProperty(stdout, "constructor", {
        value: function Socket() {},
        configurable: true
      });
    }
  }
}
