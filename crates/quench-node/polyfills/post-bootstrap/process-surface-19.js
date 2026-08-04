{
  if (globalThis.process) {
    globalThis.process.getgroups ||= () => [0];
    globalThis.process.initgroups ||= () => undefined;
    globalThis.process.setgroups ||= () => undefined;
    globalThis.process.setegid ||= () => undefined;
    globalThis.process.seteuid ||= () => undefined;
    globalThis.process.getegid ||= () => 0;
    globalThis.process.geteuid ||= () => 0;
  }
}
