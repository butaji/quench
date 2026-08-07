const __quenchOriginalRequireWithAssertStrict = globalThis.require;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "assert/strict") {
    return globalThis.__nodeAssert;
  }
  return __quenchOriginalRequireWithAssertStrict(specifier);
};
