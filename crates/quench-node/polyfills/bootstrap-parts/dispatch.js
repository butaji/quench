const __quenchRequireParts = [
  globalThis.__quench_require_part_00,
  globalThis.__quench_require_part_01,
  globalThis.__quench_require_part_02,
  globalThis.__quench_require_part_03
];
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "internal/vfs/stats" && globalThis.__quenchVfsStatsHelpers) {
    return globalThis.__quenchVfsStatsHelpers;
  }
  if (name === "internal/vfs/fd") {
    return {
      getVirtualFd(fd) {
        return globalThis.__quenchVfsFdHandles?.get(fd);
      }
    };
  }
  for (const handler of __quenchRequireParts) {
    const result = handler(name, specifier);
    if (result !== undefined) return result;
  }
  if (name.startsWith(".") || name.startsWith("/")) {
    return globalThis.__quenchLoadLocalModule(
      name,
      globalThis.__quench_script_filename || globalThis.__filename
    );
  }
  throw new Error("Cannot find module " + String(specifier));
};
