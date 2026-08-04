globalThis.__quench_bootstrap_fragments.push(
  'const __quenchProcessRef = globalThis.process;\nconst __quenchRefSymbol = Symbol.for("nodejs.ref");\nconst __quenchUnrefSymbol = Symbol.for("nodejs.unref");\n__quenchProcessRef.ref = (value) => {\n  if (value?.[__quenchRefSymbol]) value[__quenchRefSymbol]();\n  else value?.ref?.();\n};\n__quenchProcessRef.unref = (value) => {\n  if (value?.[__quenchUnrefSymbol]) value[__quenchUnrefSymbol]();\n  else value?.unref?.();\n};\n'
);
