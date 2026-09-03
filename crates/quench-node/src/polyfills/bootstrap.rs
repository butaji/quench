//! Live bootstrap fragments used by the host and Node fixture runner.
//!
//! The compatibility surface is deliberately a small ordered data set. The
//! remaining fragment files are retained as source history, but are not
//! compiled into the host until a caller adds them here.

abilities!(crate::polyfills::Phase::Bootstrap;
    "globals-extra" => globals_extra,
    "report" => report,
    "performance" => performance,
    "support" => support,
    "punycode" => punycode,
    "dns" => dns,
    "dgram-head" => dgram_head,
    "dgram" => dgram,
    "dgram-tail" => dgram_tail,
    "membership" => membership,
    "async-resource" => async_resource,
    "web-streams" => web_streams,
    "webcrypto-global" => webcrypto_global,
);
