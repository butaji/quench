//! Live bootstrap fragments used by the host and Node fixture runner.
//!
//! The compatibility surface is deliberately a small ordered data set. The
//! remaining fragment files are retained as source history, but are not
//! compiled into the host until a caller adds them here.

abilities!(crate::polyfills::Phase::Bootstrap;
    "globals-extra" => globals_extra,
    "performance" => performance,
    "support" => support,
    "dns" => dns,
    "dgram-head" => dgram_head,
    "dgram" => dgram,
    "dgram-tail" => dgram_tail,
    "membership" => membership,
    "web-streams" => web_streams,
);
