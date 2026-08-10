//! Polyfill: `alias`

pub const JS: &str = r#"const __quenchClusterAliasRequire = globalThis.require;
const __quenchClusterAlias = __quenchClusterAliasRequire("cluster");
__quenchClusterAlias.setupMaster = __quenchClusterAlias.setupPrimary;
"#;
