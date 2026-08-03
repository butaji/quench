globalThis.__quench_bootstrap_fragments.push(
  'const __quenchClusterPolicyRequire = globalThis.require;\nconst __quenchClusterPolicy = __quenchClusterPolicyRequire("cluster");\nif (__quenchClusterPolicy.schedulingPolicy === undefined) __quenchClusterPolicy.schedulingPolicy = __quenchClusterPolicy.SCHED_RR;\n'
);
