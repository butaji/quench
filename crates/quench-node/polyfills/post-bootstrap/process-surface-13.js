{
  if (
    globalThis.process &&
    globalThis.process.stdin &&
    globalThis.process.stdin.constructor.name !== "ReadStream"
  ) {
    Object.defineProperty(globalThis.process.stdin, "constructor", {
      value: function ReadStream() {},
      configurable: true
    });
  }
}
