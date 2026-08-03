globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithTestReporters = globalThis.require;\nglobalThis.require = (specifier) => {\n  if (String(specifier) === "node:test/reporters" || String(specifier) === "test/reporters") { const error = new Error(`No such built-in module: ${specifier}`); error.code = "ERR_UNKNOWN_BUILTIN_MODULE"; throw error; }\n  return __quenchOriginalRequireWithTestReporters(specifier);\n};\n'
);
