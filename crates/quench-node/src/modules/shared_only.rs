//! Shared-runtime Node modules. Values crossing these boundaries are roots
//! owned by `quench_runtime`; host policy retains Rust state and root handles.

pub(crate) mod assert {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
    pub(crate) const ASSERT_REJECTS: &str = include_str!("shared/assert_rejects.js");
}

pub(crate) mod buffer {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

#[path = "shared/buffer_enc.rs"]
pub(crate) mod buffer_enc;

#[path = "clone_shared_vm.rs"]
pub(crate) mod clone_shared_vm;

pub(crate) mod crypto {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod events {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod diagnostics_channel {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod async_hooks {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod util {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

#[path = "shared/event_loop.rs"]
pub(crate) mod event_loop;

#[path = "fetch_shared_vm.rs"]
pub(crate) mod fetch_shared_vm;

pub(crate) mod fs {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod http {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
    pub(crate) struct HttpState {
        pub(crate) shared: shared_vm::State,
    }
    pub(crate) const HTTP_METHODS: &[&str] = &[
        "ACL", "BIND", "CHECKOUT", "CONNECT", "COPY", "DELETE", "GET", "HEAD", "LINK",
        "LOCK", "M-SEARCH", "MERGE", "MKACTIVITY", "MKCALENDAR", "MKCOL", "MOVE",
        "NOTIFY", "OPTIONS", "PATCH", "POST", "PROPFIND", "PROPPATCH", "PURGE", "PUT",
        "QUERY", "REBIND", "REPORT", "SEARCH", "SOURCE", "SUBSCRIBE", "TRACE", "UNBIND",
        "UNLINK", "UNLOCK", "UNSUBSCRIBE",
    ];
    impl HttpState {
        pub(crate) fn new() -> Self {
            Self { shared: shared_vm::State::new() }
        }
    }
}

#[path = "shared/http_res.rs"]
pub(crate) mod http_res;

pub(crate) mod net {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
    pub(crate) struct NetState {
        pub(crate) shared_transport: shared_vm::Transport,
        pub(crate) auto_select_family_attempt_timeout: u64,
    }
    impl NetState {
        pub(crate) fn new() -> Self {
            Self {
                shared_transport: shared_vm::Transport::new(),
                auto_select_family_attempt_timeout: 2500,
            }
        }
        pub(crate) fn set_auto_select_family_attempt_timeout(&mut self, timeout_ms: u64) {
            self.auto_select_family_attempt_timeout = timeout_ms.max(10);
        }
    }
}

pub(crate) mod os {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
    pub(crate) fn type_str() -> String {
        if cfg!(target_os = "macos") { "Darwin" }
        else if cfg!(target_os = "linux") { "Linux" }
        else if cfg!(target_os = "windows") { "Windows_NT" }
        else { "unknown" }.to_owned()
    }
    pub(crate) fn total_memory_bytes() -> u64 {
        sysinfo::System::new_all().total_memory()
    }
}

pub(crate) mod path {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

#[path = "shared/path_algorithms.rs"]
pub(crate) mod path_algorithms;

pub mod process {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;

    pub(crate) const NODE_VERSION: &str = "22.0.0";
    pub const CHILD_RUNNER_ENV: &str = "QUENCH_CHILD_RUNNER";

    pub(crate) struct ProcessState {
        started: std::time::Instant,
        pub(crate) argv: Vec<String>,
        pub(crate) exit_code: Option<i32>,
        pub(crate) cwd: std::path::PathBuf,
        pub(crate) umask: u32,
    }
    impl ProcessState {
        pub(crate) fn new(argv: Vec<String>) -> Self {
            Self {
                started: std::time::Instant::now(),
                argv,
                exit_code: None,
                cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/")),
                umask: 0o022,
            }
        }
        pub(crate) fn uptime(&self) -> f64 { self.started.elapsed().as_secs_f64() }
        pub(crate) fn update_umask(&mut self, mask: u32) -> u32 {
            let previous = self.umask;
            self.umask = mask & 0o777;
            previous
        }
    }

    #[derive(Clone, Copy)]
    pub(crate) enum ProcessFact { Boolean(bool), Number(f64), String(&'static str), EmptyArray }

    pub(crate) fn platform() -> String {
        std::env::var("QUENCH_PLATFORM").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") { "darwin".into() }
            else if cfg!(target_os = "linux") { "linux".into() }
            else if cfg!(target_os = "windows") { "win32".into() }
            else { "unknown".into() }
        })
    }
    pub(crate) fn architecture() -> &'static str {
        if cfg!(target_arch = "x86_64") { "x64" }
        else if cfg!(target_arch = "aarch64") { "arm64" }
        else if cfg!(target_arch = "x86") { "ia32" }
        else { "unknown" }
    }
    pub(crate) fn feature_facts() -> &'static [(&'static str, ProcessFact)] {
        &[
            ("inspector", ProcessFact::Boolean(false)),
            ("debug", ProcessFact::Boolean(false)),
            ("uv", ProcessFact::Boolean(true)),
            ("ipv6", ProcessFact::Boolean(true)),
            ("openssl_is_boringssl", ProcessFact::Boolean(false)),
            ("dtls", ProcessFact::Boolean(false)),
            ("quic", ProcessFact::Boolean(false)),
            ("tls_alpn", ProcessFact::Boolean(true)),
            ("tls_sni", ProcessFact::Boolean(true)),
            ("tls_ocsp", ProcessFact::Boolean(true)),
            ("tls", ProcessFact::Boolean(true)),
            ("cached_builtins", ProcessFact::Boolean(true)),
            ("require_module", ProcessFact::Boolean(true)),
            ("typescript", ProcessFact::String("strip")),
        ]
    }
    pub(crate) fn config_variable_facts() -> &'static [(&'static str, ProcessFact)] {
        &[
            ("v8_enable_i18n_support", ProcessFact::Number(1.0)),
            ("node_module_version", ProcessFact::Number(127.0)),
            ("napi_build_version", ProcessFact::String("9")),
            ("node_builtin_shareable_builtins", ProcessFact::EmptyArray),
            ("node_use_lief", ProcessFact::Boolean(false)),
            ("node_use_amaro", ProcessFact::Boolean(false)),
            ("node_use_ffi", ProcessFact::Boolean(false)),
            ("node_shared", ProcessFact::Boolean(false)),
            ("node_shared_openssl", ProcessFact::Boolean(false)),
        ]
    }
    pub(crate) fn version_facts() -> &'static [(&'static str, &'static str)] {
        &[
            ("node", NODE_VERSION), ("acorn", "8.18.0"), ("ada", "2.7.8"),
            ("ares", "1.0.0"), ("brotli", "1.1.0"), ("cldr", "45.0"),
            ("icu", "75.1"), ("llhttp", "9.2.1"), ("modules", "127"), ("napi", "9"),
            ("openssl", "3.0.15+quench"), ("simdutf", "6.2.0"), ("tz", "2024b"),
            ("undici", "6.21.1"), ("unicode", "15.1"), ("uv", "1.48.0"),
            ("uvwasi", "0.0.21"), ("v8", "12.4.254.21-node.21"), ("zlib", "1.3.0.1-motley"),
        ]
    }
}

pub(crate) mod querystring {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

#[path = "shared/querystring_parse.rs"]
pub(crate) mod querystring_parse;

pub(crate) mod stream {
    pub(crate) const PRELUDE: &str = include_str!("stream_prelude.js");
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod string_decoder {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
    pub(crate) const MAX_STRING_BYTES: usize = 0x1fff_ffe8;
    #[derive(Clone, Copy)]
    pub(crate) enum DecodeMode { Streaming, Final }
    pub(crate) struct DecodedChunk { pub units: Vec<u16>, pub pending: Vec<u8>, pub last_total: usize }
    include!("shared/string_decoder.rs");
}

pub(crate) mod timers {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
}

pub(crate) mod url {
    #[path = "shared_vm.rs"]
    pub(crate) mod shared_vm;
    #[derive(Clone, Debug)]
    pub(crate) enum LegacyUrlField { Null, Boolean(bool), Text(String) }
    #[derive(Clone, Debug)]
    pub(crate) struct LegacyUrlParts { pub fields: Vec<(&'static str, LegacyUrlField)>, pub query: Option<String>, pub query_object: bool }
    #[derive(Clone, Debug)]
    pub(crate) enum LegacyUrlParseError { Invalid { code: String, input: String }, MalformedUri }
    include!("shared/url_parser.rs");
}
