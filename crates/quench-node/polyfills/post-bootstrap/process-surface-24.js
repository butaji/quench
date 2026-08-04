{
  if (globalThis.process) {
    globalThis.process.kill ||= () => true;
    globalThis.process.abort ||= () => undefined;
    globalThis.process.execve ||= () => undefined;
    globalThis.process.reallyExit ||= () => undefined;
  }
}
