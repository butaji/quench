globalThis.__quench_bootstrap_fragments.push(
  'const __quenchChildRefRequire = globalThis.require;\nconst __quenchChildRefModule = __quenchChildRefRequire("child_process");\nconst __quenchRefSpawn = __quenchChildRefModule.spawn;\n__quenchChildRefModule.spawn = (...args) => { const child = __quenchRefSpawn(...args); if (typeof child.ref !== "function") child.ref = () => undefined; return child; };\n'
);
