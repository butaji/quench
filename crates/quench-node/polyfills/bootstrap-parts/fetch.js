Object.defineProperty(globalThis, "fetch", {
  value: async () => {
    throw new Error("fetch is not implemented");
  },
  configurable: true,
  writable: true,
  enumerable: false,
});
