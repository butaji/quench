{
  if (globalThis.process) {
    const usage = (globalThis.process.resourceUsage ||= () => ({}));
    const sample = usage();
    for (const name of "ipcReceived ipcSent sharedMemorySize signalsCount swappedOut unsharedDataSize unsharedStackSize".split(
      " "
    )) {
      sample[name] ??= 0;
    }
    const memory = globalThis.process.memoryUsage();
    for (const name of "arrayBuffers external heapTotal heapUsed rss".split(
      " "
    )) {
      memory[name] ??= 0;
    }
  }
}
