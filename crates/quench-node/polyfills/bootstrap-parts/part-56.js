globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithSqlite = globalThis.require;\nglobalThis.require = (specifier) => {\n  if (String(specifier) === "node:sqlite" || String(specifier) === "sqlite") {\n    const error = new Error(`No such built-in module: ${specifier}`);\n    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";\n    throw error;\n  }\n  return __quenchOriginalRequireWithSqlite(specifier);\n};\n'
);
