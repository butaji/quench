globalThis.__quench_bootstrap_fragments.push(
  'const __quenchChildDisposeRequire = globalThis.require;\nconst __quenchChildDisposeModule = __quenchChildDisposeRequire("child_process");\nconst __quenchChildDisposeSpawn = __quenchChildDisposeModule.spawn;\n__quenchChildDisposeModule.spawn = (...args) => { const child = __quenchChildDisposeSpawn(...args); child.destroy = (error) => { child.kill(error ? "SIGTERM" : "SIGTERM"); return child; }; child[Symbol.dispose] = () => { child.kill("SIGTERM"); }; return child; };\n'
);
