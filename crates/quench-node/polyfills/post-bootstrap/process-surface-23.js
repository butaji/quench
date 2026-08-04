{
  if (globalThis.process) {
    globalThis.process._getActiveHandles ||= () => [];
    globalThis.process._getActiveRequests ||= () => [];
  }
}
