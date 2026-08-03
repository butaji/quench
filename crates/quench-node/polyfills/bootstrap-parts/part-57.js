globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithSharedUtil = globalThis.require;\nconst __quenchSharedUtil = __quenchOriginalRequireWithSharedUtil("util");\nglobalThis.require = (specifier) => { const name = String(specifier).replace(/^node:/, ""); if (name === "util" || name === "sys") return __quenchSharedUtil; return __quenchOriginalRequireWithSharedUtil(specifier); };\n'
);
