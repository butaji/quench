globalThis.__quench_bootstrap_fragments.push(
  'const __quenchClusterCleanupRequire = globalThis.require;\nconst __quenchClusterCleanup = __quenchClusterCleanupRequire("cluster");\nconst __quenchOriginalWorkerKill = __quenchClusterCleanup.Worker?.prototype.kill;\nif (__quenchOriginalWorkerKill) __quenchClusterCleanup.Worker.prototype.kill = function (...args) { const result = __quenchOriginalWorkerKill.apply(this, args); if (__quenchClusterCleanup.workers) delete __quenchClusterCleanup.workers[this.id]; return result; };\n'
);
