{
  if (globalThis.process) {
    globalThis.process._rawDebug ||= (...args) => {
      let message = String(args.shift() ?? "");
      for (const value of args) message = message.replace("%s", String(value));
      globalThis.process.stderr?.write?.(`${message}\n`);
    };
    globalThis.process._debugProcess ||= () => undefined;
    globalThis.process._debugEnd ||= () => undefined;
    globalThis.process._startProfilerIdleNotifier ||= () => undefined;
    globalThis.process._stopProfilerIdleNotifier ||= () => undefined;
    globalThis.process._tickCallback ||= () => undefined;
  }
}
