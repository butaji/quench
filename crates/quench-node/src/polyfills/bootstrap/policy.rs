//! Polyfill: `policy`

pub const JS: &str = r#"const __quenchClusterPolicyRequire = globalThis.require;
const __quenchClusterPolicy = __quenchClusterPolicyRequire("cluster");
if (__quenchClusterPolicy.schedulingPolicy === undefined) {
  __quenchClusterPolicy.schedulingPolicy = __quenchClusterPolicy.SCHED_RR;
}
"#;
