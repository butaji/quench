globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithAssertStrict = globalThis.require;\nglobalThis.require = (specifier) => {\n  if (String(specifier).replace(/^node:/, "") === "assert/strict") return globalThis.__nodeAssert;\n  return __quenchOriginalRequireWithAssertStrict(specifier);\n};\n'
);
