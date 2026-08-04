{
  if (globalThis.process) {
    globalThis.process._rawDebug ||= () => undefined;
    globalThis.process._debugProcess ||= () => undefined;
    globalThis.process._debugEnd ||= () => undefined;
    globalThis.process._startProfilerIdleNotifier ||= () => undefined;
    globalThis.process._stopProfilerIdleNotifier ||= () => undefined;
    globalThis.process._tickCallback ||= () => undefined;
  }
}
