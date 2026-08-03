globalThis.__quench_bootstrap_fragments.push(
  'const __quenchClusterSetupRequire = globalThis.require;\nconst __quenchClusterSetup = __quenchClusterSetupRequire("cluster");\nconst __quenchOriginalSetupPrimary = __quenchClusterSetup.setupPrimary;\n__quenchClusterSetup.setupPrimary = (options = {}) => __quenchOriginalSetupPrimary({ ...__quenchClusterSetup.settings, ...options });\n'
);
