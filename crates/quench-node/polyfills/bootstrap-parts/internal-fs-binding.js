globalThis.__quenchInternalFsBinding = {
  openFileHandle: (_path, _flags, _mode, _req, _context) => undefined,
  readdir: (path) => {
    const names = globalThis.__nodeFs.readdirSync(path);
    return [names, names.map(() => 1)];
  },
};
globalThis.__quenchInternalFallbackBinding = { fstat: () => undefined };
globalThis.__quenchInternalBindingCore = (binding) => {
  if (binding === "buffer") {
    return {
      fill: (buffer, offset, end, value, encoding) => {
        if (
          !Number.isInteger(offset) ||
          !Number.isInteger(end) ||
          offset < 0 ||
          end < offset ||
          end > buffer.length ||
          (typeof value === "number" && (value < 0 || value > 255))
        ) {
          const error = new RangeError("value out of range");
          error.code = "ERR_OUT_OF_RANGE";
          throw error;
        }
        return buffer.fill(value, offset, end, encoding);
      },
    };
  }
  if (binding === "fs") {
    return Object.assign(globalThis.__quenchInternalFsBinding, {
      fstat: (fd) => globalThis.__nodeFs.fstatSync(fd),
    });
  }
  if (binding === "os") return globalThis.__quenchInternalOsBinding;
  if (binding === "debug") {
    return {
      getGenericUsageCount: (name) =>
        name.includes("Uninitialized")
          ? __nodeAllocatorCounts.uninitialized
          : __nodeAllocatorCounts.zeroFilled,
    };
  }
};
