const __quenchChildStreamStateRequire = globalThis.require;
const __quenchChildStreamStateModule = __quenchChildStreamStateRequire(
  "child_process",
);
const __quenchChildStreamStateSpawn = __quenchChildStreamStateModule.spawn;
__quenchChildStreamStateModule.spawn = (...args) => {
  const child = __quenchChildStreamStateSpawn(...args);
  Object.assign(child.stdin, {
    readable: false,
    writable: true,
    destroyed: false,
  });
  Object.assign(child.stdout, {
    readable: true,
    writable: true,
    destroyed: false,
  });
  Object.assign(child.stderr, {
    readable: true,
    writable: true,
    destroyed: false,
  });
  return child;
};
