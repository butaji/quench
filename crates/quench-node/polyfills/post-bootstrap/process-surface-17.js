{
  if (globalThis.process) {
    const dispose = async () => undefined;
    globalThis.process.stdout[Symbol.asyncDispose] ||= dispose;
    globalThis.process.stderr[Symbol.asyncDispose] ||= dispose;
  }
}
