globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithWasi = globalThis.require;\nglobalThis.require = (specifier) => {\n  if (String(specifier).replace(/^node:/, "") === "wasi") {\n    const error = new Error(`No such built-in module: ${specifier}`);\n    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";\n    throw error;\n  }\n  return __quenchOriginalRequireWithWasi(specifier);\n};\n'
);
