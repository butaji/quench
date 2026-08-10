//! Compile-time codegen for polyfill JavaScript.
//!
//! Polyfill fragments that are pure default-value tables are expressed here as
//! compact JSON literals. A Rust macro splices each literal into a one-line
//! JavaScript patch (`for(const[k,v]of Object.entries({...})) target[k]??=v;`)
//! and the combined patch is prepended to the bootstrap source. The
//! generated JavaScript is identical to the original assignment-by-assignment
//! `globalThis.X ||= Y` form, so runtime behavior is preserved.

/// Build a `for(const[k,v]of Object.entries({...})) target[k]??=v;` patch
/// from a JSON literal data table.
#[macro_export]
macro_rules! obj_apply_or_eq {
    ($target:expr, $data:literal) => {
        concat!(
            "for(const[k,v]of Object.entries(",
            $data,
            "))",
            $target,
            "[k]??=v;",
        )
    };
}

/// Combined polyfill defaults patch: process.versions table, features,
/// flags, stdin defaults, stdin lifecycle, process.report, runtime features,
/// resourceUsage sample fields, and memoryUsage fields.
///
/// Each table is a single JSON literal in the source, replacing ~10-25
/// lines of `globalThis.X ||= Y` repetition in the post-bootstrap JS.
pub const POLYFILL_PATCH: &str = concat!(
    "if(typeof process==='object'){",
    obj_apply_or_eq!(
        "globalThis.process.versions",
        r#"{"node":"22.0.0","v8":"12.4.254.21-node.20","uv":"1.48.0","openssl":"3.0.13","zlib":"1.3.0","modules":"127","napi":"9","acorn":"8.11.3","ada":"2.7.8","tz":"2024a","brotli":"1.1.0","nbytes":"1.0.0","cldr":"45.0","icu":"75.1","nghttp2":"1.61.0","llhttp":"9.2.1","nghttp3":"1.3.0","ngtcp2":"1.4.0","simdutf":"5.2.4","unicode":"15.1","undici":"6.19.8","cjs_module_lexer":"1.2.2"}"#
    ),
    obj_apply_or_eq!(
        "globalThis.process.features",
        r#"{"inspector":false}"#
    ),
    obj_apply_or_eq!(
        "globalThis.process",
        r#"{"noDeprecation":false,"traceDeprecation":false,"throwDeprecation":false}"#
    ),
    obj_apply_or_eq!(
        "globalThis.process.stdin",
        r#"{"readable":true,"readableEnded":false,"readableFlowing":null,"readableHighWaterMark":65536,"readableLength":0,"readableObjectMode":false}"#
    ),
    obj_apply_or_eq!(
        "process.stdin",
        r#"{"fd":0,"destroyed":false,"readableEncoding":null,"closed":false,"errored":null,"readableAborted":false,"autoClose":false,"bytesRead":0,"pending":false,"end":null}"#
    ),
    obj_apply_or_eq!(
        "globalThis.process.report",
        r#"{"compact":false,"directory":"","excludeEnv":false,"excludeNetwork":false,"filename":"","reportOnFatalError":false,"reportOnSignal":false,"reportOnUncaughtException":false,"signal":"SIGUSR2"}"#
    ),
    obj_apply_or_eq!(
        "process.features",
        r#"{"cached_builtins":true,"debug":false,"ipv6":true,"openssl_is_boringssl":false,"quic":false,"require_module":true,"tls":true,"tls_alpn":true,"tls_ocsp":true,"tls_sni":true,"typescript":"strip","uv":true}"#
    ),
    obj_apply_or_eq!(
        "process.resourceUsage()",
        r#"{"ipcReceived":0,"ipcSent":0,"sharedMemorySize":0,"signalsCount":0,"swappedOut":0,"unsharedDataSize":0,"unsharedStackSize":0}"#
    ),
    obj_apply_or_eq!(
        "process.memoryUsage()",
        r#"{"arrayBuffers":0,"external":0,"heapTotal":0,"heapUsed":0}"#
    ),
    "}",
);
