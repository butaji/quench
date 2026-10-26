//! Shared-runtime Node host state and entry configuration.

use std::cell::RefCell;
use std::rc::Rc;

use crate::shared_run::EntryGoal;

/// Output callback owned by the canonical Node host boundary.
pub type NodeOutputSink = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone, Copy, Default)]
pub(crate) enum InlineSource {
    #[default]
    Eval,
    Stdin,
}
impl InlineSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Eval => "[eval]",
            Self::Stdin => "[stdin]",
        }
    }
    pub(crate) fn compilation_name(self) -> &'static str {
        match self {
            Self::Eval => "<eval>",
            Self::Stdin => "[stdin]",
        }
    }
}

pub struct NodeHost {
    /// Immutable installation asset root, independent of guest cwd changes.
    pub(crate) node_library_root: std::path::PathBuf,
    pub(crate) shared_state: Rc<RefCell<SharedNodeState>>,
    pub(crate) commonjs_entry: Option<CommonJsEntry>,
    pub(crate) inline_source: InlineSource,
    pub(crate) eval_builtin_globals: bool,
    /// Node's default argument slice uses the normalized, nonempty eval source.
    pub(crate) eval_source_nonempty: bool,
    pub(crate) initial_node_debug: Option<String>,
    pub(crate) net_pending_writes: Vec<(u64, Vec<u8>, bool)>,
    pub(crate) net_pending_ends: Vec<(u64, bool)>,
    pub(crate) net_pending_destroys: Vec<(u64, bool)>,
    /// In-memory source records supplied by `vm.SourceTextModule.link()` for
    /// the runtime's native ESM linker. Keys are (referrer, specifier).
    pub(crate) vm_module_sources:
        std::collections::HashMap<(String, String), quench_runtime::ModuleSource>,
}

#[derive(Clone)]
pub(crate) struct CommonJsEntry {
    pub(crate) path: std::path::PathBuf,
    pub(crate) goal: EntryGoal,
}

/// Before node:module is built the loader owns its cache directly. Thereafter
/// Module._cache is authoritative, including guest replacement of that property.
#[derive(Clone, Copy)]
pub(crate) enum CommonJsCache {
    Bootstrap(quench_runtime::RootId),
    Module(quench_runtime::RootId),
}

/// Host-owned state and lifecycle data for the shared VM path.
pub(crate) struct SharedNodeState {
    pub(crate) workers: crate::modules::worker_threads_shared_vm::State,
    pub(crate) vm_scripts: crate::modules::contextify_shared_vm::State,
    pub(crate) async_hooks: crate::modules::async_hooks_state::SharedAsyncHooksState,
    pub(crate) native_tasks: crate::modules::native_task_pool::Pool,
    #[cfg(quench_node_cares)]
    pub(crate) cares: crate::modules::cares_channel_shared_vm::State,
    pub(crate) dns: crate::modules::cares_binding::State,
    pub(crate) performance_native: crate::modules::performance_binding_shared_vm::State,
    pub(crate) scheduler: crate::modules::shared_event_loop::SharedEventLoop,
    pub(crate) fs: crate::modules::fs_state::FsState,
    pub(crate) fs_watch: crate::modules::fs_watch_shared_vm::State,
    pub(crate) tls_contexts: crate::modules::tls_shared_vm::contexts::State,
    #[cfg(quench_icu)]
    pub(crate) icu_converters: crate::modules::icu_binding_shared_vm::State,
    pub(crate) fs_stats: crate::modules::fs_shared_vm::stat::types::State,
    pub(crate) fs_policy: crate::modules::fs_policy_shared_vm::State,
    pub(crate) fs_stat_watch: crate::modules::fs_stat_watch_shared_vm::State,
    pub(crate) cwd: crate::modules::process_state::ProcessCwd,
    pub(crate) buffer_prototype: Option<quench_runtime::RootId>,
    pub(crate) module_parent_api: Option<quench_runtime::RootId>,
    pub(crate) active_esm_requires: std::collections::HashMap<String, quench_runtime::RootId>,
    pub(crate) module_require_trace_active: bool,
    pub(crate) builtin_cache: std::collections::HashMap<String, quench_runtime::RootId>,
    pub(crate) commonjs_cache: Option<CommonJsCache>,
    /// Node's internal primordial snapshot used when loading `lib/internal/*`.
    pub(crate) primordials_module: Option<quench_runtime::RootId>,
    pub(crate) realm_module_programs: std::collections::HashMap<
        std::path::PathBuf,
        crate::host::shared_vm::commonjs::PreparedRealmModule,
    >,
    pub(crate) crypto_handle_symbol: Option<quench_runtime::RootId>,
    pub(crate) event_targets_api: Option<quench_runtime::RootId>,
    pub(crate) trace: crate::modules::trace_state::TraceState,
    pub(crate) util_intrinsics: Option<crate::modules::util_shared_vm::UtilIntrinsics>,
    pub(crate) node_error_api: Option<crate::modules::node_errors_shared_vm::NodeErrorApi>,
    /// Immutable startup arguments used to build guest process.argv.
    pub(crate) process_argv: crate::modules::process_state::ProcessArgs,
    pub(crate) process_control: crate::modules::process_state::ProcessControl,
    pub(crate) process_signals: crate::modules::process_signals_state::SignalState,
    pub(crate) stdin: crate::modules::process_stdin_shared_vm::State,
    pub(crate) process_ipc: crate::modules::child_process_ipc_shared_vm::State,
    pub(crate) zlib_streams: crate::modules::zlib_stream_shared_vm::State,
    #[cfg(quench_openssl_provider)]
    pub(crate) crypto_hashes: crate::modules::crypto_hash_shared_vm::State,
    pub(crate) crypto_key_material: crate::modules::crypto_key_material_shared_vm::State,
    #[cfg(quench_openssl_provider)]
    pub(crate) crypto_signatures: crate::modules::crypto_signature_shared_vm::State,
    pub(crate) crypto_secret_jobs: crate::modules::crypto_secret_job_shared_vm::State,
    pub(crate) zlib_native: crate::modules::zlib_native_shared_vm::State,
    pub(crate) unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode,
    pub(crate) output: Option<NodeOutputSink>,
    /// Optional stdout-only sink. Worker stdio pipes use this to keep stdout
    /// separate from diagnostic and stderr output.
    pub(crate) stdout_output: Option<NodeOutputSink>,
    pub(crate) net_auto_select_family_attempt_timeout: u64,
    pub(crate) net_auto_select_family: bool,
    pub(crate) process_module: Option<quench_runtime::RootId>,
    pub(crate) process_capture_state: Option<quench_runtime::RootId>,
    pub(crate) path_module: Option<quench_runtime::RootId>,
    pub(crate) url_constructor: Option<quench_runtime::RootId>,
    pub(crate) url_state: Option<quench_runtime::RootId>,
    pub(crate) url_search_params_constructor: Option<quench_runtime::RootId>,
    pub(crate) timer_handle_api: Option<crate::modules::timers_shared_vm::TimerHandleApi>,
    pub(crate) timer_duration_warnings: crate::modules::timers_shared_vm::TimerDurationWarnings,
    /// Invocation flags supplied by the embedder for this logical process.
    pub(crate) exec_argv: Vec<String>,
    pub(crate) cli_options: crate::modules::cli_options::CliOptions,
    pub(crate) report_config: crate::modules::report_configuration_shared_vm::State,
    /// Per-worker process.env snapshot, when Worker({ env }) supplied one.
    pub(crate) process_env_override: Option<std::collections::HashMap<String, String>>,
    /// Whether this isolate participates in worker_threads.SHARE_ENV.
    pub(crate) process_env_shared: bool,
    initial_node_options: Option<String>,
    initial_pending_deprecation: bool,
    pub(crate) fetch: crate::modules::fetch_shared_vm::FetchState,
    pub(crate) diagnostics: crate::modules::diagnostics_channel_shared_vm::SharedDiagnosticsState,
    pub(crate) http: crate::modules::http_shared_vm::State,
    pub(crate) tcp: crate::modules::net_shared_vm::Transport,
    pub(crate) net_sockets: std::collections::HashMap<u64, NetSocket>,
    pub(crate) net_closing_sockets: std::collections::HashMap<u64, quench_runtime::RootId>,
    pub(crate) net_socket_close_completed: Option<quench_runtime::RootId>,
    pub(crate) udp: crate::modules::udp_binding::State,
    pub(crate) native_async_wrap: crate::modules::native_async_wrap::State,
    pub(crate) native_pipe: crate::modules::pipe_binding::State,
    pub(crate) native_stream: crate::modules::stream_binding::State,
    pub(crate) native_tcp_listeners: std::collections::HashMap<u64, quench_runtime::RootId>,
    pub(crate) net_servers: std::collections::HashMap<u64, NetServer>,
    pub(crate) child_process_shell_args_warning_emitted: bool,
    pub(crate) child_processes: std::collections::HashMap<u64, SharedChildProcess>,
    pub(crate) net_socket_constructor: Option<quench_runtime::RootId>,
    pub(crate) net_handle_owner_symbol: Option<quench_runtime::RootId>,
    pub(crate) net_server_on_connection: Option<quench_runtime::RootId>,
    pub(crate) net_handle_onclose_symbol: Option<quench_runtime::RootId>,
    pub(crate) net_socket_option_keys: Option<quench_runtime::RootId>,
    pub(crate) net_handle_identities: Vec<(quench_runtime::HostWeakRoot, u64)>,
    pub(crate) net_handle_close_callbacks: std::collections::HashMap<u64, quench_runtime::RootId>,
}

pub(crate) struct NetSocket {
    pub(crate) root: quench_runtime::RootId,
    pub(crate) encoding: Option<String>,
    pub(crate) parent_server: Option<u64>,
    pub(crate) write_callbacks: std::collections::VecDeque<quench_runtime::RootId>,
}

pub(crate) struct NetServer {
    pub(crate) root: quench_runtime::RootId,
    pub(crate) listener: u64,
    pub(crate) connections: std::collections::HashSet<u64>,
    pub(crate) closing: bool,
    pub(crate) listening_pending: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildHandlePhase {
    Open,
    Closing,
    Closed,
}

pub(crate) struct SharedChildProcess {
    pub(crate) cancel: crate::modules::child_process_shared_vm::Cancellation,
    pub(crate) handle_phase: ChildHandlePhase,
    pub(crate) referenced: bool,
    pub(crate) close_dispatched: bool,
    pub(crate) child: Option<quench_runtime::RootId>,
    pub(crate) stdout: Option<quench_runtime::RootId>,
    pub(crate) stderr: Option<quench_runtime::RootId>,
    #[cfg(unix)]
    pub(crate) stdout_pipe: crate::modules::child_process_output_shared_vm::Output,
    #[cfg(unix)]
    pub(crate) stderr_pipe: crate::modules::child_process_output_shared_vm::Output,
    pub(crate) stdin_stream: Option<quench_runtime::RootId>,
    pub(crate) stdin: crate::modules::child_process_input_shared_vm::Input,
    #[cfg(unix)]
    pub(crate) extra_inputs: Vec<(usize, crate::modules::child_process_input_shared_vm::Input)>,
    #[cfg(unix)]
    pub(crate) extra_outputs: Vec<(
        usize,
        crate::modules::child_process_output_shared_vm::Output,
    )>,
    pub(crate) ipc: Option<crate::modules::child_process_ipc_shared_vm::IpcChannel>,
    pub(crate) ipc_pipe: Option<u64>,
    pub(crate) events: std::sync::mpsc::Receiver<ChildProcessEvent>,
    pub(crate) stdout_terminal: bool,
    pub(crate) stderr_terminal: bool,
    pub(crate) exit: Option<(Option<i32>, Option<String>)>,
    pub(crate) exit_event_dispatched: bool,
}

pub(crate) enum ChildProcessEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    ExtraStdout(usize, Vec<u8>),
    StdoutEnd,
    StderrEnd,
    ExtraStdoutEnd(usize),
    Exit(Option<i32>, Option<String>),
}

impl SharedNodeState {
    /// Forget cached guest handles before bootstrapping a replacement VM heap.
    pub(crate) fn reset_cached_vm_roots(&mut self) {
        self.performance_native = Default::default();
        self.vm_scripts = Default::default();
        self.builtin_cache.clear();
        self.commonjs_cache = None;
        self.module_parent_api = None;
        self.active_esm_requires.clear();
        self.module_require_trace_active = false;
        self.buffer_prototype = None;
        self.workers.reset_vm_roots();
        self.primordials_module = None;
        self.crypto_handle_symbol = None;
        self.event_targets_api = None;
        self.process_module = None;
        self.process_capture_state = None;
        self.path_module = None;
        self.url_constructor = None;
        self.url_state = None;
        self.url_search_params_constructor = None;
        self.timer_handle_api = None;
        self.timer_duration_warnings = Default::default();
        self.net_socket_constructor = None;
        self.net_handle_owner_symbol = None;
        self.net_server_on_connection = None;
        self.net_handle_onclose_symbol = None;
        self.net_socket_option_keys = None;
        self.udp = Default::default();
        self.native_async_wrap = Default::default();
        self.native_pipe = Default::default();
        self.native_stream = Default::default();
        self.native_tcp_listeners.clear();
        self.net_handle_identities.clear();
        self.net_handle_close_callbacks.clear();
        self.fs_watch = crate::modules::fs_watch_shared_vm::State::default();
        self.tls_contexts = crate::modules::tls_shared_vm::contexts::State::default();
        #[cfg(quench_icu)]
        {
            self.icu_converters = crate::modules::icu_binding_shared_vm::State::default();
        }
        self.fs_stats = crate::modules::fs_shared_vm::stat::types::State::default();
        self.fs_policy = crate::modules::fs_policy_shared_vm::State::default();
        self.fs_stat_watch = crate::modules::fs_stat_watch_shared_vm::State::default();
    }

    fn new(
        fs: crate::modules::fs_state::FsState,
        cwd: crate::modules::process_state::ProcessCwd,
        process_argv: crate::modules::process_state::ProcessArgs,
        process_control: crate::modules::process_state::ProcessControl,
        async_identity: crate::modules::async_hooks_state::AsyncIdentity,
    ) -> Self {
        Self {
            workers: Default::default(),
            vm_scripts: Default::default(),
            async_hooks: crate::modules::async_hooks_state::SharedAsyncHooksState::new(
                async_identity,
            ),
            native_tasks: Default::default(),
            #[cfg(quench_node_cares)]
            cares: Default::default(),
            dns: Default::default(),
            performance_native: Default::default(),
            scheduler: crate::modules::shared_event_loop::SharedEventLoop::new(),
            fs,
            fs_watch: crate::modules::fs_watch_shared_vm::State::default(),
            tls_contexts: crate::modules::tls_shared_vm::contexts::State::default(),
            #[cfg(quench_icu)]
            icu_converters: crate::modules::icu_binding_shared_vm::State::default(),
            fs_stats: crate::modules::fs_shared_vm::stat::types::State::default(),
            fs_policy: crate::modules::fs_policy_shared_vm::State::default(),
            fs_stat_watch: crate::modules::fs_stat_watch_shared_vm::State::default(),
            cwd,
            buffer_prototype: None,
            builtin_cache: std::collections::HashMap::new(),
            commonjs_cache: None,
            module_parent_api: None,
            active_esm_requires: std::collections::HashMap::new(),
            module_require_trace_active: false,
            primordials_module: None,
            realm_module_programs: std::collections::HashMap::new(),
            crypto_handle_symbol: None,
            event_targets_api: None,
            util_intrinsics: None,
            node_error_api: None,
            trace: crate::modules::trace_state::TraceState::default(),
            process_argv,
            process_control,
            process_signals: crate::modules::process_signals_state::SignalState::default(),
            stdin: crate::modules::process_stdin_shared_vm::State::default(),
            process_ipc: crate::modules::child_process_ipc_shared_vm::State::default(),
            zlib_streams: crate::modules::zlib_stream_shared_vm::State::default(),
            #[cfg(quench_openssl_provider)]
            crypto_hashes: crate::modules::crypto_hash_shared_vm::State::default(),
            crypto_key_material: crate::modules::crypto_key_material_shared_vm::State::default(),
            #[cfg(quench_openssl_provider)]
            crypto_signatures: crate::modules::crypto_signature_shared_vm::State::default(),
            crypto_secret_jobs: crate::modules::crypto_secret_job_shared_vm::State::default(),
            zlib_native: crate::modules::zlib_native_shared_vm::State::default(),
            unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode::Throw,
            output: None,
            stdout_output: None,
            net_auto_select_family: crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY,
            net_auto_select_family_attempt_timeout:
                crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT,
            process_module: None,
            process_capture_state: None,
            path_module: None,
            url_constructor: None,
            url_state: None,
            url_search_params_constructor: None,
            timer_handle_api: None,
            timer_duration_warnings: Default::default(),
            exec_argv: Vec::new(),
            cli_options: Default::default(),
            report_config: Default::default(),
            process_env_override: None,
            process_env_shared: false,
            initial_node_options: std::env::var("NODE_OPTIONS").ok(),
            initial_pending_deprecation: std::env::var("NODE_PENDING_DEPRECATION").as_deref()
                == Ok("1"),
            fetch: crate::modules::fetch_shared_vm::FetchState::new(),
            diagnostics:
                crate::modules::diagnostics_channel_shared_vm::SharedDiagnosticsState::default(),
            http: crate::modules::http_shared_vm::State::new(),
            tcp: crate::modules::net_shared_vm::Transport::new(),
            net_sockets: std::collections::HashMap::new(),
            net_closing_sockets: std::collections::HashMap::new(),
            net_socket_close_completed: None,
            udp: Default::default(),
            native_async_wrap: Default::default(),
            native_pipe: Default::default(),
            native_stream: Default::default(),
            native_tcp_listeners: std::collections::HashMap::new(),
            net_servers: std::collections::HashMap::new(),
            child_process_shell_args_warning_emitted: false,
            child_processes: std::collections::HashMap::new(),
            net_socket_constructor: None,
            net_handle_owner_symbol: None,
            net_server_on_connection: None,
            net_handle_onclose_symbol: None,
            net_socket_option_keys: None,
            net_handle_identities: Vec::new(),
            net_handle_close_callbacks: std::collections::HashMap::new(),
        }
    }
}

impl NodeHost {
    pub fn new(argv: Vec<String>) -> Self {
        let node_library_root = std::env::var_os(crate::shared_run::NODE_LIBRARY_ROOT_ENV)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|parent| parent.join("node")))
                    .unwrap_or_else(|| std::path::PathBuf::from("node"))
            });
        let node_library_root = if node_library_root.is_absolute() {
            node_library_root
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("/"))
                .join(node_library_root)
        };
        let node_library_root = node_library_root
            .canonicalize()
            .unwrap_or(node_library_root);
        // The Node test common/tmpdir helper exposes a per-process host path.
        // Materialize that parent at host construction so fixtures can create
        // files there even when the helper's JS-side refresh hook is absent
        // from a reduced bootstrap realm.
        let tmp = std::path::PathBuf::from(format!("/tmp/quench-node-{}", std::process::id()));
        let _ = std::fs::create_dir_all(tmp);
        let fs = crate::modules::fs_state::FsState::new();
        let argv = crate::modules::process_state::ProcessArgs::new(argv);
        let async_identity = crate::modules::async_hooks_state::AsyncIdentity::new();
        let process_control = crate::modules::process_state::ProcessControl::new();
        let cwd = crate::modules::process_state::ProcessCwd::from_current_directory();
        Self {
            node_library_root,
            shared_state: Rc::new(RefCell::new(SharedNodeState::new(
                fs,
                cwd,
                argv,
                process_control,
                async_identity,
            ))),
            commonjs_entry: None,
            inline_source: InlineSource::default(),
            eval_builtin_globals: false,
            eval_source_nonempty: false,
            initial_node_debug: std::env::var_os("NODE_DEBUG")
                .map(|value| value.to_string_lossy().into_owned()),
            net_pending_writes: Vec::new(),
            net_pending_ends: Vec::new(),
            net_pending_destroys: Vec::new(),
            vm_module_sources: std::collections::HashMap::new(),
        }
    }

    /// Select Node's Script/Module parse goal from its filename and package type.
    pub fn source_kind(path: &std::path::Path) -> Result<quench_runtime::SourceKind, String> {
        super::shared_vm::source_kind(path)
    }

    /// The last VFS CLI switch owns feature availability for this logical process.
    pub(crate) fn vfs_enabled(&self) -> bool {
        self.shared_state
            .borrow()
            .cli_options
            .enabled("--experimental-vfs")
    }

    /// Execute a file through the CommonJS loader during shared-VM initialization.
    pub fn with_commonjs_entry(mut self, path: std::path::PathBuf) -> Self {
        self.commonjs_entry = Some(CommonJsEntry {
            path,
            goal: EntryGoal::Node,
        });
        self
    }

    pub fn with_commonjs_entry_goal(mut self, path: std::path::PathBuf, goal: EntryGoal) -> Self {
        self.commonjs_entry = Some(CommonJsEntry { path, goal });
        self
    }

    /// Retain the Node invocation flags supplied by the fixture adapter.
    pub fn with_exec_argv(self, exec_argv: Vec<String>) -> Result<Self, String> {
        let (cwd, node_options) = {
            let shared = self.shared_state.borrow();
            (shared.cwd.path(), shared.initial_node_options.clone())
        };
        crate::modules::util_env::load_startup_env_files(node_options.as_deref(), &exec_argv, &cwd);
        let mode =
            crate::modules::process_state::UnhandledRejectionMode::from_exec_argv(&exec_argv)?;
        let net_timeout =
            crate::modules::net_config::auto_select_family_attempt_timeout_from_exec_argv(
                &exec_argv,
            );
        let net_family = crate::modules::net_config::auto_select_family_from_exec_argv(&exec_argv);
        let mut shared = self.shared_state.borrow_mut();
        let node_options = std::env::var("NODE_OPTIONS")
            .ok()
            .or_else(|| shared.initial_node_options.clone());
        shared.cli_options = crate::modules::cli_options::CliOptions::parse(
            node_options.as_deref(),
            &exec_argv,
            shared.initial_pending_deprecation,
        )?;
        shared.initial_node_options = std::env::var("NODE_OPTIONS").ok();
        shared.report_config = crate::modules::report_configuration_shared_vm::State::from_options(
            &shared.cli_options,
        );
        shared.trace.configure(&exec_argv);
        shared.exec_argv = exec_argv;
        shared.unhandled_rejection_mode = mode;
        if let Some(enabled) = net_family {
            shared.net_auto_select_family = enabled;
        }
        if let Some(net_timeout) = net_timeout {
            shared.net_auto_select_family_attempt_timeout = net_timeout;
        }
        drop(shared);
        Ok(self)
    }

    pub fn with_output_sink(self, sink: NodeOutputSink) -> Self {
        self.shared_state.borrow_mut().output = Some(sink);
        self
    }

    pub fn with_stdout_sink(self, sink: NodeOutputSink) -> Self {
        self.shared_state.borrow_mut().stdout_output = Some(sink);
        self
    }

    pub fn with_initial_node_options(self, options: Option<String>) -> Self {
        self.shared_state.borrow_mut().initial_node_options = options;
        self
    }

    pub(crate) fn shared_state(&self) -> Rc<RefCell<SharedNodeState>> {
        self.shared_state.clone()
    }

    /// Exit code recorded by `process.exit`, if any.
    pub fn exit_code(&self) -> Option<i32> {
        self.shared_state.borrow().process_control.exit_code()
    }
}
