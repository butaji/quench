//! Live bootstrap fragments used by the host and Node fixture runner.
//!
//! The compatibility surface is deliberately a small ordered data set. The
//! remaining fragment files are retained as source history, but are not
//! compiled into the host until a caller adds them here.

#[path = "bootstrap/cluster.rs"]
pub mod cluster;
#[path = "bootstrap/iterators.rs"]
pub mod iterators;

abilities!(crate::polyfills::Phase::Bootstrap;
    "globals-extra" => globals_extra,
    "fetch" => fetch,
    "externalizable-strings" => externalizable_strings,
    "report" => report,
    "performance" => performance,
    "support" => support,
    "event-emitter" => event_emitter,
    "punycode" => punycode,
    "dns" => dns,
    "dgram-head" => dgram_head,
    "dgram" => dgram,
    "dgram-tail" => dgram_tail,
    "membership" => membership,
    "async-resource" => async_resource,
    "web-streams" => web_streams,
    "webcrypto-global" => webcrypto_global,
    "vfs-head" => vfs_head,
    "vfs" => vfs,
);

/// Installed Node globals shared by file, eval and embedded entry points.
pub fn entry_globals_source() -> String {
    [
        web_streams::JS,
        performance::JS,
        r#"
Object.defineProperty(globalThis, "URL", { value: URL, writable: true, configurable: true });
Object.defineProperty(globalThis, "__nodeURL", { value: globalThis.URL, configurable: true });
Object.defineProperty(globalThis, "__nodeURLSearchParams", { value: globalThis.URLSearchParams, configurable: true });
"#,
    ]
    .join("\n")
}
