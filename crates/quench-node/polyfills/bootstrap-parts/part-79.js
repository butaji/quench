globalThis.__quench_bootstrap_fragments.push(
  'const __quenchForkChildRequire = globalThis.require;\nconst __quenchForkChildModule = __quenchForkChildRequire("child_process");\nif (__quenchForkChildModule._forkChild === undefined) __quenchForkChildModule._forkChild = (fd, options) => undefined;\n'
);
