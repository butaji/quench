//! Polyfill: `cluster-runtime`

pub const JS: &str = r#"const __quenchClusterRequire = globalThis.require;
const __quenchCluster = __quenchClusterRequire("cluster");
__quenchCluster.SCHED_NONE = 1;
__quenchCluster.SCHED_RR = 2;
"#;
