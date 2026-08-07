{
  if (globalThis.process && globalThis.process.stderr) {
    const stderr = globalThis.process.stderr;
    stderr.writableHighWaterMark = 65536;
    if (stderr.constructor.name !== "Socket") {
      Object.defineProperty(stderr, "constructor", {
        value: function Socket() {},
        configurable: true,
      });
    }
  }
}
