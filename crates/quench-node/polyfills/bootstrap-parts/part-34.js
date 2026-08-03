globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithSys = globalThis.require;\nglobalThis.require = (specifier) => {\n  if (String(specifier).replace(/^node:/, "") === "sys") return __quenchOriginalRequireWithSys("util");\n  return __quenchOriginalRequireWithSys(specifier);\n};\n'
);
