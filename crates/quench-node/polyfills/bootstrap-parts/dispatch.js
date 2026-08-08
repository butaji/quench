const __quenchRequireParts = [
  globalThis.__quench_require_part_00,
  globalThis.__quench_require_part_01,
  globalThis.__quench_require_part_02,
  globalThis.__quench_require_part_03
];
globalThis.require = (specifier) => {
  const rawName = String(specifier);
  const name = rawName.startsWith("node:") ? rawName.slice(5) : rawName;
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
  if (name === "internal/vfs/router") {
    const path = globalThis.__nodePath;
    return {
      isUnderMountPoint(value, mountPoint) {
        const valuePath = path.resolve(value);
        const mountPath = path.resolve(mountPoint);
        return (
          mountPath === path.parse(mountPath).root ||
          valuePath === mountPath ||
          valuePath.startsWith(`${mountPath}${path.sep}`)
        );
      },
      getRelativePath(value, mountPoint) {
        const relative = path.relative(
          path.resolve(mountPoint),
          path.resolve(value)
        );
        return relative ? `/${relative.split(path.sep).join("/")}` : "/";
      },
      isAbsolutePath: path.isAbsolute
    };
  }
  if (name === "worker_threads") {
    return { isMainThread: true, MessageChannel, MessagePort };
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
