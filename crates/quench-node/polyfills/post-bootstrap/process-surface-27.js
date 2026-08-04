{
  if (globalThis.process) {
    globalThis.process.ref ||= () => undefined;
    globalThis.process.unref ||= () => undefined;
  }
}
