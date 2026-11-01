//! Shared-runtime Node host state and entry configuration.

use std::cell::RefCell;
use std::rc::Rc;

use crate::shared_run::EntryGoal;

mod snapshot;

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
    /// Compiled library modules kept across runs (`compile_cache`).
    pub(crate) compile_cache: crate::host::compile_cache::CompileCache,
    /// Whether bootstrap has finished (`Host::initialize`, or `Host::resume` from a startup
    /// snapshot): bootstrap's own programs decode back to back (`cached_program_decoded`).
    pub(crate) bootstrapped: bool,
    pub(crate) shared_state: Rc<RefCell<SharedNodeState>>,
    pub(crate) commonjs_entry: Option<CommonJsEntry>,
    pub(crate) inline_source: InlineSource,
    pub(crate) eval_builtin_globals: bool,
    /// Node's default argument slice uses the normalized, nonempty eval source.
    pub(crate) eval_source_nonempty: bool,
    pub(crate) initial_node_debug: Option<String>,
    pub(crate) net_pending_writes: Vec<(u64, Vec<u8>)>,
    pub(crate) net_pending_ends: Vec<u64>,
    pub(crate) net_pending_destroys: Vec<(u64, bool)>,
    /// The kind of startup snapshot this host builds, ending `initialize` at its point.
    pub(crate) building_snapshot: Option<crate::host::startup_snapshot::SnapshotKind>,
    /// Each referrer's resolved import specifiers (`NodeHost::import_location`).
    pub(crate) resolved_imports: rustc_hash::FxHashMap<
        Box<str>,
        rustc_hash::FxHashMap<Box<str>, Rc<crate::host::shared_vm::ImportLocation>>,
    >,
    /// Allocated only for applications that opt into hot module reloading.
    pub(crate) hmr_graph: Option<crate::host::hmr::ModuleGraph>,
    /// CLI HMR uses process replacement when this runtime cannot apply an edit in place.
    pub(crate) hmr_restart_on_change: bool,
    /// Worker modules and loaded module metadata that require restarting the CLI process.
    hmr_restart_only_paths: rustc_hash::FxHashSet<std::path::PathBuf>,
    /// Shared from the main host to worker hosts, which publish loaded source stamps.
    hmr_worker_watch_registry: Option<crate::host::hmr::WorkerWatchRegistry>,
    /// Whether this host publishes its watched application paths to its parent process.
    hmr_publish_worker_paths: bool,
    /// The active `import.meta.hot` object by canonical source path.
    pub(crate) hmr_hot_modules: rustc_hash::FxHashMap<std::path::PathBuf, quench_runtime::RootId>,
    /// Compiler source identities for each watched file; multiple URLs require restart fallback.
    pub(crate) hmr_source_names:
        rustc_hash::FxHashMap<std::path::PathBuf, rustc_hash::FxHashSet<String>>,
    /// `import.meta.hot.data` waiting for the replacement program's metadata setup.
    pub(crate) hmr_hot_data: rustc_hash::FxHashMap<std::path::PathBuf, quench_runtime::RootId>,
    /// Initial TypeScript programs waiting for background checker queries.
    pub(crate) hmr_pending_type_queries:
        Vec<crate::host::hmr::type_worker::TypeQueryRequest>,
    /// Started on the first TypeScript HMR update; checker work stays off the event loop.
    pub(crate) hmr_type_worker: Option<crate::host::hmr::type_worker::TypeWorker>,
}

impl NodeHost {
    /// `path`'s real path through the loaders' known-real paths (`real_path`); a path that
    /// cannot be resolved is returned as it is.
    pub(crate) fn real_path(&mut self, path: std::path::PathBuf) -> std::path::PathBuf {
        let real = crate::host::real_path::real_path(&path).unwrap_or_else(|_| path.clone());
        self.record_hmr_resolution_symlinks(&path, &real);
        real
    }

    /// A loaded module keeps its canonical path for Node identity, but HMR also needs to
    /// observe links used to reach it: swapping a workspace package link can change future
    /// resolution while leaving the loaded target's bytes untouched. Link changes always
    /// restart, since this process has already resolved the module through the old target.
    pub(crate) fn record_hmr_resolution_symlinks(
        &mut self,
        requested: &std::path::Path,
        resolved: &std::path::Path,
    ) {
        if self.hmr_graph.is_none() || !self.hmr_graph_path(requested) {
            return;
        }
        let mut current = std::path::PathBuf::new();
        let mut links = Vec::new();
        for component in requested.components() {
            current.push(component.as_os_str());
            if !matches!(component, std::path::Component::Normal(_)) {
                continue;
            }
            let is_symlink = std::fs::symlink_metadata(&current)
                .is_ok_and(|metadata| metadata.file_type().is_symlink());
            let resolves_to_dependency = is_symlink
                && crate::host::real_path::real_path(&current).is_ok_and(|real_link| {
                    resolved.starts_with(&real_link) || real_link.starts_with(resolved)
                });
            if resolves_to_dependency && self.hmr_watch_path(&current) {
                links.push(current.clone());
            }
        }
        if links.is_empty() {
            return;
        }
        if let Some(graph) = &mut self.hmr_graph {
            for link in &links {
                graph.record_module(link.clone());
                graph.record_dependency(resolved.to_path_buf(), link.clone(), None);
            }
        }
        if self.hmr_publish_worker_paths {
            self.publish_worker_hmr_paths(links.iter().map(std::path::PathBuf::as_path));
        }
        self.hmr_restart_only_paths.extend(links);
    }

    fn record_hmr_package_symlinks(
        &mut self,
        importer: &std::path::Path,
        dependency: &std::path::Path,
        specifier: &str,
    ) {
        if specifier.starts_with('.')
            || std::path::Path::new(specifier).is_absolute()
            || specifier.starts_with("node:")
            || specifier.starts_with("internal/")
        {
            return;
        }
        let mut components = std::path::Path::new(specifier).components();
        let Some(first) = components.next() else {
            return;
        };
        let mut package = std::path::PathBuf::from(first.as_os_str());
        if first.as_os_str().to_string_lossy().starts_with('@') {
            let Some(second) = components.next() else {
                return;
            };
            package.push(second.as_os_str());
        }
        for directory in importer.ancestors().skip(1) {
            let candidate = directory.join("node_modules").join(&package);
            let Ok(real_candidate) = crate::host::real_path::real_path(&candidate) else {
                continue;
            };
            if dependency.starts_with(&real_candidate) {
                self.record_hmr_resolution_symlinks(&candidate, dependency);
                return;
            }
        }
    }
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
    pub(crate) module_parent_api: Option<quench_runtime::RootId>,
    pub(crate) active_esm_requires: std::collections::HashMap<String, quench_runtime::RootId>,
    pub(crate) builtin_cache: std::collections::HashMap<String, quench_runtime::RootId>,
    /// Buffer's allocation pool while the host holds it for `buffer.js` to adopt.
    pub(crate) buffer_pool: crate::modules::buffer_shared_vm::PoolOwner,
    pub(crate) commonjs_cache: Option<CommonJsCache>,
    /// Node's internal primordial snapshot used when loading `lib/internal/*`.
    pub(crate) primordials_module: Option<quench_runtime::RootId>,
    pub(crate) realm_module_programs: std::collections::HashMap<
        std::path::PathBuf,
        crate::host::shared_vm::commonjs::PreparedRealmModule,
    >,
    pub(crate) crypto_handle_symbol: Option<quench_runtime::RootId>,
    /// Library methods as their modules defined them (`commonjs::LibraryOriginal`).
    pub(crate) library_originals: crate::host::shared_vm::commonjs::LibraryOriginals,
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
    #[cfg(quench_openssl_provider)]
    pub(crate) crypto_hashes: crate::modules::crypto_hash_shared_vm::State,
    pub(crate) crypto_key_material: crate::modules::crypto_key_material_shared_vm::State,
    #[cfg(quench_openssl_provider)]
    pub(crate) crypto_signatures: crate::modules::crypto_signature_shared_vm::State,
    pub(crate) crypto_secret_jobs: crate::modules::crypto_secret_job_shared_vm::State,
    pub(crate) zlib_native: crate::modules::zlib_native_shared_vm::State,
    pub(crate) messaging: crate::modules::worker_shared_vm::State,
    /// Pure-Rust resolver channels for `cares_wrap` when c-ares is not linked.
    #[cfg(not(quench_node_cares))]
    pub(crate) dns_channels: crate::modules::dns_shared_vm::State,
    pub(crate) serdes: crate::modules::serdes_shared_vm::State,
    pub(crate) unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode,
    pub(crate) output: Option<NodeOutputSink>,
    pub(crate) net_auto_select_family_attempt_timeout: u64,
    pub(crate) net_auto_select_family: bool,
    pub(crate) process_module: Option<quench_runtime::RootId>,
    pub(crate) process_capture_state: Option<quench_runtime::RootId>,
    pub(crate) path_module: Option<quench_runtime::RootId>,
    pub(crate) timer_handle_api: Option<crate::modules::timers_shared_vm::TimerHandleApi>,
    /// The emitter configuration's fixed keys and constructor (`event_constructor_shared_vm`).
    pub(crate) emitter_keys: Option<crate::modules::event_constructor_shared_vm::EmitterKeys>,
    /// The internal invocation every receiverless next tick shares (`queue_next_tick`).
    pub(crate) tick_invocation: Option<quench_runtime::RootId>,
    /// The string `"error"`, which every EventEmitter `emit` compares its event with.
    pub(crate) error_event_name: Option<quench_runtime::RootId>,
    pub(crate) timer_duration_warnings: crate::modules::timers_shared_vm::TimerDurationWarnings,
    /// Invocation flags supplied by the embedder for this logical process.
    pub(crate) exec_argv: Vec<String>,
    pub(crate) cli_options: crate::modules::cli_options::CliOptions,
    pub(crate) report_config: crate::modules::report_configuration_shared_vm::State,
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
    pub(crate) ipc: Option<crate::modules::child_process_ipc_shared_vm::IpcChannel>,
    pub(crate) ipc_pipe: Option<u64>,
    pub(crate) events: std::sync::mpsc::Receiver<ChildProcessEvent>,
    /// Held until the loop delivers the child's exit; dropping it lets the waiter reap the
    /// child, which stays signalable as a zombie until then, as under Node.
    pub(crate) reap: Option<std::sync::mpsc::Sender<()>>,
    pub(crate) stdout_terminal: bool,
    pub(crate) stderr_terminal: bool,
    pub(crate) exit: Option<(Option<i32>, Option<String>)>,
}

pub(crate) enum ChildProcessEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    StdoutEnd,
    StderrEnd,
    Exit(Option<i32>, Option<String>),
}

impl SharedNodeState {
    /// Forget cached guest handles before bootstrapping a replacement VM heap.
    pub(crate) fn reset_cached_vm_roots(&mut self) {
        self.performance_native = Default::default();
        self.vm_scripts = Default::default();
        self.builtin_cache.clear();
        self.buffer_pool = Default::default();
        self.messaging.reset();
        #[cfg(not(quench_node_cares))]
        self.dns_channels.reset();
        self.serdes.reset();
        self.commonjs_cache = None;
        self.module_parent_api = None;
        self.active_esm_requires.clear();
        self.workers.reset_vm_roots();
        self.primordials_module = None;
        self.crypto_handle_symbol = None;
        self.library_originals = Default::default();
        self.event_targets_api = None;
        self.process_module = None;
        self.process_capture_state = None;
        self.path_module = None;
        self.timer_handle_api = None;
        self.tick_invocation = None;
        self.error_event_name = None;
        self.emitter_keys = None;
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
            builtin_cache: std::collections::HashMap::new(),
            buffer_pool: Default::default(),
            commonjs_cache: None,
            module_parent_api: None,
            active_esm_requires: std::collections::HashMap::new(),
            primordials_module: None,
            realm_module_programs: std::collections::HashMap::new(),
            crypto_handle_symbol: None,
            library_originals: Default::default(),
            event_targets_api: None,
            util_intrinsics: None,
            node_error_api: None,
            trace: crate::modules::trace_state::TraceState::default(),
            process_argv,
            process_control,
            process_signals: crate::modules::process_signals_state::SignalState::default(),
            stdin: crate::modules::process_stdin_shared_vm::State::default(),
            process_ipc: crate::modules::child_process_ipc_shared_vm::State::default(),
            #[cfg(quench_openssl_provider)]
            crypto_hashes: crate::modules::crypto_hash_shared_vm::State::default(),
            crypto_key_material: crate::modules::crypto_key_material_shared_vm::State::default(),
            #[cfg(quench_openssl_provider)]
            crypto_signatures: crate::modules::crypto_signature_shared_vm::State::default(),
            crypto_secret_jobs: crate::modules::crypto_secret_job_shared_vm::State::default(),
            zlib_native: crate::modules::zlib_native_shared_vm::State::default(),
            messaging: crate::modules::worker_shared_vm::State::default(),
            #[cfg(not(quench_node_cares))]
            dns_channels: crate::modules::dns_shared_vm::State::default(),
            serdes: crate::modules::serdes_shared_vm::State::default(),
            unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode::Throw,
            output: None,
            net_auto_select_family: crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY,
            net_auto_select_family_attempt_timeout:
                crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT,
            process_module: None,
            process_capture_state: None,
            path_module: None,
            timer_handle_api: None,
            tick_invocation: None,
            error_event_name: None,
            emitter_keys: None,
            timer_duration_warnings: Default::default(),
            exec_argv: Vec::new(),
            cli_options: Default::default(),
            report_config: Default::default(),
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
        // Under the test runner, materialize that parent at host construction so
        // fixtures can create files there even when the helper's JS-side refresh
        // hook is absent from a reduced bootstrap realm. Other runs create nothing:
        // every process had left an empty directory in /tmp.
        if std::env::var_os(crate::shared_run::CHILD_RUNNER_ENV).is_some() {
            let tmp = std::path::PathBuf::from(format!("/tmp/quench-node-{}", std::process::id()));
            let _ = std::fs::create_dir_all(tmp);
        }
        let fs = crate::modules::fs_state::FsState::new();
        let argv = crate::modules::process_state::ProcessArgs::new(argv);
        let async_identity = crate::modules::async_hooks_state::AsyncIdentity::new();
        let process_control = crate::modules::process_state::ProcessControl::new();
        let cwd = crate::modules::process_state::ProcessCwd::from_current_directory();
        Self {
            node_library_root,
            compile_cache: crate::host::compile_cache::CompileCache::from_environment(),
            bootstrapped: false,
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
            resolved_imports: Default::default(),
            hmr_graph: None,
            hmr_restart_on_change: false,
            hmr_restart_only_paths: Default::default(),
            hmr_worker_watch_registry: None,
            hmr_publish_worker_paths: false,
            hmr_hot_modules: Default::default(),
            hmr_source_names: Default::default(),
            hmr_hot_data: Default::default(),
            hmr_pending_type_queries: Vec::new(),
            hmr_type_worker: None,
            net_pending_writes: Vec::new(),
            net_pending_ends: Vec::new(),
            net_pending_destroys: Vec::new(),
            building_snapshot: None,
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

    /// Enable the HMR module graph for one application entry.
    pub fn with_hmr_entry(self, path: std::path::PathBuf) -> Self {
        self.with_hmr_entry_stamp(path, None)
    }

    pub(crate) fn with_hmr_entry_file_stamp(
        self,
        path: std::path::PathBuf,
        stamp: crate::host::hmr::FileStamp,
    ) -> Self {
        self.with_hmr_entry_stamp(path, Some(stamp))
    }

    fn with_hmr_entry_stamp(
        mut self,
        path: std::path::PathBuf,
        stamp: Option<crate::host::hmr::FileStamp>,
    ) -> Self {
        let requested_path = path.clone();
        let path = self.real_path(path);
        let stamp = stamp.or_else(|| crate::host::hmr::file_stamp(&path));
        self.hmr_graph = Some(crate::host::hmr::ModuleGraph::new_with_stamp(path.clone(), stamp));
        self.hmr_worker_watch_registry.get_or_insert_with(Default::default);
        self.record_hmr_resolution_symlinks(&requested_path, &path);
        self.record_hmr_module(&path);
        self
    }

    pub(crate) fn with_hmr_worker_watch_registry(
        mut self,
        registry: crate::host::hmr::WorkerWatchRegistry,
    ) -> Self {
        self.hmr_worker_watch_registry = Some(registry);
        self.hmr_publish_worker_paths = true;
        self
    }

    pub(crate) fn with_hmr_worker_graph(mut self) -> Self {
        if self.hmr_graph.is_none() {
            self.hmr_graph = Some(crate::host::hmr::ModuleGraph::new_worker());
        }
        self
    }

    pub(crate) fn hmr_worker_watch_registry(
        &self,
    ) -> Option<crate::host::hmr::WorkerWatchRegistry> {
        self.hmr_graph
            .as_ref()
            .and(self.hmr_worker_watch_registry.clone())
    }

    pub(crate) fn hmr_restart_only(&self, path: &std::path::Path) -> bool {
        self.hmr_restart_only_paths.contains(path)
    }

    pub(crate) fn with_hmr_restart_on_change(mut self) -> Self {
        self.hmr_restart_on_change = true;
        self
    }

    pub(crate) fn hmr_restart_on_change(&self) -> bool {
        self.hmr_restart_on_change
    }

    pub(crate) fn record_hmr_source_name(
        &mut self,
        path: &std::path::Path,
        source_name: &str,
    ) {
        if self.hmr_graph.is_none() || !self.hmr_path(path) {
            return;
        }
        let path = self.real_path(path.to_path_buf());
        if self.hmr_path(&path) {
            self.hmr_source_names
                .entry(path)
                .or_default()
                .insert(source_name.to_owned());
        }
    }

    pub(crate) fn hmr_source_name(&self, path: &std::path::Path) -> Option<&str> {
        let names = self.hmr_source_names.get(path)?;
        if names.len() != 1 {
            return None;
        }
        names.iter().next().map(String::as_str)
    }

    pub(crate) fn hmr_source_name_is_ambiguous(&self, path: &std::path::Path) -> bool {
        self.hmr_source_names
            .get(path)
            .is_some_and(|names| names.len() > 1)
    }

    pub(crate) fn submit_hmr_type_queries(
        &mut self,
        path: &std::path::Path,
        source_name: &str,
        function_start_offset: u32,
        positions: &[crate::host::shared_vm::source_transform::TypeQueryPosition],
    ) -> Result<(), String> {
        let path = path.to_path_buf();
        self.hmr_pending_type_queries
            .retain(|request| request.path != path);
        if let Some(worker) = &mut self.hmr_type_worker {
            worker.invalidate(&path);
        }
        if positions.is_empty() {
            return Ok(());
        }
        if self
            .hmr_type_worker
            .as_ref()
            .is_some_and(|worker| !worker.has_capacity())
        {
            self.queue_hmr_type_queries(&path, source_name, function_start_offset, positions);
            return Ok(());
        }
        let worker = match &mut self.hmr_type_worker {
            Some(worker) => worker,
            None => {
                self.hmr_type_worker = Some(crate::host::hmr::type_worker::TypeWorker::start()?);
                self.hmr_type_worker.as_mut().expect("worker was just installed")
            }
        };
        worker.submit(&path, source_name, function_start_offset, positions)
    }

    pub(crate) fn queue_hmr_type_queries(
        &mut self,
        path: &std::path::Path,
        source_name: &str,
        function_start_offset: u32,
        positions: &[crate::host::shared_vm::source_transform::TypeQueryPosition],
    ) {
        if positions.is_empty() || !self.hmr_restart_on_change {
            return;
        }
        let request = crate::host::hmr::type_worker::TypeQueryRequest {
            path: path.to_path_buf(),
            source_name: source_name.to_owned(),
            function_start_offset,
            positions: positions.to_vec(),
        };
        if let Some(previous) = self
            .hmr_pending_type_queries
            .iter_mut()
            .find(|previous| previous.path == request.path)
        {
            *previous = request;
        } else {
            self.hmr_pending_type_queries.push(request);
        }
    }

    pub(crate) fn take_hmr_type_queries(
        &mut self,
    ) -> Vec<crate::host::hmr::type_worker::TypeQueryRequest> {
        std::mem::take(&mut self.hmr_pending_type_queries)
    }

    pub(crate) fn poll_hmr_type_queries(
        &mut self,
    ) -> Vec<Result<crate::host::hmr::type_worker::TypeWorkerUpdate, String>> {
        let results = self.hmr_type_worker
            .as_mut()
            .map(crate::host::hmr::type_worker::TypeWorker::poll)
            .unwrap_or_default();
        if results.iter().any(Result::is_err) {
            self.hmr_type_worker = None;
        }
        results
    }

    /// The normalized application entry when HMR graph tracking is enabled.
    pub fn hmr_entry(&self) -> Option<&std::path::Path> {
        self.hmr_graph
            .as_ref()
            .map(crate::host::hmr::ModuleGraph::entry)
    }

    pub(crate) fn hmr_hot_module(&self, path: &std::path::Path) -> Option<quench_runtime::RootId> {
        self.hmr_hot_modules.get(path).copied()
    }

    pub(crate) fn take_hmr_hot_module(
        &mut self,
        path: &std::path::Path,
    ) -> Option<quench_runtime::RootId> {
        self.hmr_hot_modules.remove(path)
    }

    pub(crate) fn replace_hmr_hot_module(
        &mut self,
        path: std::path::PathBuf,
        hot: quench_runtime::RootId,
    ) -> Option<quench_runtime::RootId> {
        self.hmr_hot_modules.insert(path, hot)
    }

    pub(crate) fn replace_hmr_hot_data(
        &mut self,
        path: std::path::PathBuf,
        data: quench_runtime::RootId,
    ) -> Option<quench_runtime::RootId> {
        self.hmr_hot_data.insert(path, data)
    }

    pub(crate) fn take_hmr_hot_data(
        &mut self,
        path: &std::path::Path,
    ) -> Option<quench_runtime::RootId> {
        self.hmr_hot_data.remove(path)
    }

    /// Loaded application modules affected by `changed` paths, including their importers.
    pub fn hmr_invalidated_by(
        &self,
        changed: impl IntoIterator<Item = std::path::PathBuf>,
    ) -> Vec<std::path::PathBuf> {
        self.hmr_graph
            .as_ref()
            .map(|graph| graph.invalidated_by(changed))
            .unwrap_or_default()
    }

    /// Polls watched module files and returns both changed files and their importer closure.
    pub fn poll_hmr_changes(&mut self) -> Option<crate::host::hmr::HmrChangeSet> {
        let registry = self.hmr_worker_watch_registry.clone();
        let worker_paths = registry
            .map(|registry| {
                let mut registry = registry
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                std::mem::take(&mut *registry)
            })
            .unwrap_or_default();
        for (path, stamp) in worker_paths {
            if !self.hmr_watch_path(&path) {
                continue;
            }
            let Some(graph) = &mut self.hmr_graph else {
                continue;
            };
            graph.record_module_with_stamp(path.clone(), stamp);
            self.hmr_restart_only_paths.insert(path);
        }
        let graph = self.hmr_graph.as_mut()?;
        let changed = graph.changed_files(std::time::Instant::now());
        let invalidated = graph.invalidated_by(changed.iter().cloned());
        Some(crate::host::hmr::HmrChangeSet {
            changed,
            invalidated,
        })
    }

    pub(crate) fn retry_hmr_changes(
        &mut self,
        paths: impl IntoIterator<Item = std::path::PathBuf>,
    ) {
        if let Some(graph) = &mut self.hmr_graph {
            graph.retry_changes(paths);
        }
    }

    pub(crate) fn hmr_change_matches_stamp(
        &self,
        path: &std::path::Path,
        prepared_digest: Option<&crate::host::hmr::FileContentDigest>,
    ) -> bool {
        self.hmr_graph
            .as_ref()
            .is_some_and(|graph| graph.change_matches_stamp(path, prepared_digest))
    }

    pub(crate) fn record_hmr_module(&mut self, path: &std::path::Path) {
        if self.hmr_graph.is_none() || !self.hmr_graph_path(path) {
            return;
        }
        let path = self.real_path(path.to_path_buf());
        if !self.hmr_graph_path(&path) {
            return;
        }
        let watch_source = self.hmr_path(&path);
        let mut metadata_paths = Vec::new();
        for package in crate::host::shared_vm::commonjs::package_scope_watch_paths(&path) {
            let package = self.real_path(package);
            if self.hmr_watch_path(&package) {
                metadata_paths.push(package);
            }
        }
        if watch_source {
            for config in crate::host::shared_vm::source_transform::tsconfig_watch_paths(&path) {
                let config = self.real_path(config);
                if self.hmr_watch_path(&config) {
                    metadata_paths.push(config);
                }
            }
        }
        if let Some(graph) = &mut self.hmr_graph {
            if watch_source {
                graph.record_module(path.clone());
            } else {
                graph.record_graph_module(path.clone());
            }
            for metadata in &metadata_paths {
                graph.record_dependency(path.clone(), metadata.clone(), None);
                graph.record_module(metadata.clone());
            }
        }
        self.hmr_restart_only_paths
            .extend(metadata_paths.iter().cloned());
        if self.hmr_publish_worker_paths {
            let paths = std::iter::once(path.as_path())
                .chain(metadata_paths.iter().map(std::path::PathBuf::as_path));
            self.publish_worker_hmr_paths(paths);
        }
    }

    pub(crate) fn hmr_tracks_path(&mut self, path: &std::path::Path) -> bool {
        if self.hmr_graph.is_none() || !self.hmr_path(path) {
            return false;
        }
        let path = self.real_path(path.to_path_buf());
        self.hmr_path(&path)
    }

    pub(crate) fn record_hmr_dependency(
        &mut self,
        importer: &std::path::Path,
        dependency: &std::path::Path,
        specifier: Option<&str>,
    ) {
        if self.hmr_graph.is_none()
            || !self.hmr_graph_path(importer)
            || !self.hmr_graph_path(dependency)
        {
            return;
        }
        let dependency = self.real_path(dependency.to_path_buf());
        if !self.hmr_graph_path(&dependency) {
            return;
        }
        let importer = self.real_path(importer.to_path_buf());
        if !self.hmr_graph_path(&importer) {
            return;
        }

        self.record_hmr_module(&dependency);
        self.record_hmr_module(&importer);
        if let Some(specifier) = specifier {
            self.record_hmr_package_symlinks(&importer, &dependency, specifier);
        }
        if let Some(graph) = &mut self.hmr_graph {
            graph.record_dependency(importer, dependency, specifier);
        }
    }

    fn publish_worker_hmr_paths<'a>(&mut self, paths: impl IntoIterator<Item = &'a std::path::Path>) {
        let Some(registry) = &self.hmr_worker_watch_registry else {
            return;
        };
        let Some(graph) = &self.hmr_graph else {
            return;
        };
        let mut published = Vec::new();
        for path in paths {
            let Some(stamp) = graph.stamp(path) else {
                continue;
            };
            published.push((path.to_path_buf(), stamp));
        }
        let mut registry = registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        registry.extend(published);
    }

    pub(crate) fn hmr_direct_importers(
        &self,
        dependency: &std::path::Path,
    ) -> Vec<(std::path::PathBuf, Vec<String>)> {
        self.hmr_graph
            .as_ref()
            .map(|graph| graph.direct_importers(dependency))
            .unwrap_or_default()
    }

    pub(crate) fn begin_hmr_module_reload(
        &mut self,
        path: &std::path::Path,
    ) -> Vec<(std::path::PathBuf, Vec<String>)> {
        let Some(graph) = &mut self.hmr_graph else {
            return Vec::new();
        };
        graph.take_dependencies(path)
    }

    pub(crate) fn restore_hmr_module_dependencies(
        &mut self,
        path: &std::path::Path,
        dependencies: Vec<(std::path::PathBuf, Vec<String>)>,
    ) {
        if let Some(graph) = &mut self.hmr_graph {
            graph.restore_dependencies(path, dependencies);
        }
    }

    fn hmr_path(&self, path: &std::path::Path) -> bool {
        self.hmr_graph_path(path)
            && !path
                .components()
                .any(|component| component.as_os_str() == "node_modules")
    }

    fn hmr_graph_path(&self, path: &std::path::Path) -> bool {
        path.is_absolute() && !path.starts_with(&self.node_library_root)
    }

    fn hmr_watch_path(&self, path: &std::path::Path) -> bool {
        path.is_absolute() && !path.starts_with(&self.node_library_root)
    }

    /// Retain the Node invocation flags supplied by the fixture adapter.
    pub fn with_exec_argv(self, exec_argv: Vec<String>) -> Result<Self, String> {
        let mode =
            crate::modules::process_state::UnhandledRejectionMode::from_exec_argv(&exec_argv)?;
        let net_timeout =
            crate::modules::net_config::auto_select_family_attempt_timeout_from_exec_argv(
                &exec_argv,
            );
        let net_family = crate::modules::net_config::auto_select_family_from_exec_argv(&exec_argv);
        let mut shared = self.shared_state.borrow_mut();
        shared.cli_options = crate::modules::cli_options::CliOptions::parse(
            shared.initial_node_options.as_deref(),
            &exec_argv,
            shared.initial_pending_deprecation,
        )?;
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

    pub(crate) fn shared_state(&self) -> Rc<RefCell<SharedNodeState>> {
        self.shared_state.clone()
    }

    /// Exit code recorded by `process.exit`, if any.
    pub fn exit_code(&self) -> Option<i32> {
        self.shared_state.borrow().process_control.exit_code()
    }
}
