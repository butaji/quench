globalThis.__quench_bootstrap_fragments.push(
  'const __quenchOriginalRequireWithUrlStatics = globalThis.require;\nconst __quenchUrlParse = (input, base) => {\n  if (base === undefined && !/^[A-Za-z][A-Za-z0-9+.-]*:/.test(String(input))) return null;\n  try { return new URL(input, base); } catch { return null; }\n};\nif (typeof URL.canParse !== "function") URL.canParse = (input, base) => __quenchUrlParse(input, base) !== null;\nif (typeof URL.parse !== "function") URL.parse = __quenchUrlParse;\nglobalThis.require = (specifier) => __quenchOriginalRequireWithUrlStatics(specifier);\n'
);
