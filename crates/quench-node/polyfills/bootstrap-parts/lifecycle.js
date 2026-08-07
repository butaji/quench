const __quenchChildLifecycleRequire = globalThis.require;
const __quenchChildLifecycle = __quenchChildLifecycleRequire("child_process");
const __quenchLifecycleSpawn = __quenchChildLifecycle.spawn;
__quenchChildLifecycle.spawn = (...args) => {
  const child = __quenchLifecycleSpawn(...args);
  return child;
};
