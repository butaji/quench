//! Polyfill: `alias`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchClusterAliasRequire = globalThis.require;
const __quenchClusterAlias = __quenchClusterAliasRequire("cluster");
__quenchClusterAlias.setupMaster = __quenchClusterAlias.setupPrimary;
"#);
