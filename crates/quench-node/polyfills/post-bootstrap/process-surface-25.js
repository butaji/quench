{
  if (globalThis.process) {
    globalThis.process.binding ||= () => ({});
    globalThis.process._linkedBinding ||= () => ({});
    globalThis.process.dlopen ||= () => undefined;
  }
}
