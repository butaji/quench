globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithInspector = globalThis.require;\nglobalThis.require = (specifier) => {\n  const name = String(specifier).replace(/^node:/, "");\n  if (name === "inspector" || name === "inspector/promises") {\n    const error = new Error(`No such built-in module: ${specifier}`);\n    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";\n    throw error;\n  }\n  return __quenchOriginalRequireWithInspector(specifier);\n};\n'
);
