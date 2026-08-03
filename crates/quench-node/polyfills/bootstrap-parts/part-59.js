globalThis.__quench_bootstrap_fragments.push(
  'const __quenchClusterApiRequire = globalThis.require;\nconst __quenchClusterApi = __quenchClusterApiRequire("cluster");\nif (typeof __quenchClusterApi.Worker?.prototype.isConnected !== "function") __quenchClusterApi.Worker.prototype.isConnected = function () { return this.state !== "dead" && this.state !== "disconnected"; };\n'
);
