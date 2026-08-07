const __quenchChildProcessConstructorRequire = globalThis.require;
const __quenchChildProcessConstructor = __quenchChildProcessConstructorRequire(
  "child_process",
);
if (__quenchChildProcessConstructor.ChildProcess === undefined) {
  __quenchChildProcessConstructor.ChildProcess = globalThis.__nodeEventEmitter;
}
