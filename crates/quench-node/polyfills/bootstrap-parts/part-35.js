globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithTraceEvents = globalThis.require;\nglobalThis.require = (specifier) => {\n  if (String(specifier).replace(/^node:/, "") === "trace_events") {\n    const error = new Error(`No such built-in module: ${specifier}`);\n    error.code = "ERR_UNKNOWN_BUILTIN_MODULE";\n    throw error;\n  }\n  return __quenchOriginalRequireWithTraceEvents(specifier);\n};\n'
);
