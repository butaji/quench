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
    pub(crate) net_pending_writes: Vec<(u64, Vec<u8>)>,
    pub(crate) net_pending_ends: Vec<u64>,
    pub(crate) net_pending_destroys: Vec<(u64, bool)>,
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
    pub(crate) async_hooks: crate::modules::async_hooks_state::SharedAsyncHooksState,
    pub(crate) scheduler: crate::modules::shared_event_loop::SharedEventLoop,
    pub(crate) fs: crate::modules::fs_state::FsState,
    pub(crate) fs_watch: crate::modules::fs_watch_shared_vm::State,
    pub(crate) tls_contexts: crate::modules::tls_shared_vm::contexts::State,
    pub(crate) fs_stats: crate::modules::fs_shared_vm::stat::types::State,
    pub(crate) fs_stat_watch: crate::modules::fs_stat_watch_shared_vm::State,
    pub(crate) cwd: crate::modules::process_state::ProcessCwd,
    pub(crate) buffer_prototype: Option<quench_runtime::RootId>,
    pub(crate) module_parent_api: Option<quench_runtime::RootId>,
    pub(crate) active_esm_requires: std::collections::HashMap<String, quench_runtime::RootId>,
    pub(crate) builtin_cache: std::collections::HashMap<String, quench_runtime::RootId>,
    pub(crate) commonjs_cache: Option<CommonJsCache>,
    /// Node's internal primordial snapshot used when loading `lib/internal/*`.
    pub(crate) primordials_module: Option<quench_runtime::RootId>,
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
    pub(crate) stream_high_water_marks: crate::modules::stream_state::HighWaterMarks,
    pub(crate) unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode,
    pub(crate) output: Option<NodeOutputSink>,
    pub(crate) net_auto_select_family_attempt_timeout: u64,
    pub(crate) net_auto_select_family: bool,
    pub(crate) process_module: Option<quench_runtime::RootId>,
    pub(crate) assert_module: Option<quench_runtime::RootId>,
    pub(crate) path_module: Option<quench_runtime::RootId>,
    pub(crate) url_constructor: Option<quench_runtime::RootId>,
    pub(crate) url_state: Option<quench_runtime::RootId>,
    pub(crate) url_search_params_constructor: Option<quench_runtime::RootId>,
    pub(crate) timer_handle_api: Option<crate::modules::timers_shared_vm::TimerHandleApi>,
    pub(crate) timer_duration_warnings: crate::modules::timers_shared_vm::TimerDurationWarnings,
    /// Invocation flags supplied by the embedder for this logical process.
    pub(crate) exec_argv: Vec<String>,
    pub(crate) fetch: crate::modules::fetch_shared_vm::FetchState,
    pub(crate) diagnostics: crate::modules::diagnostics_channel_shared_vm::SharedDiagnosticsState,
    pub(crate) http: crate::modules::http_shared_vm::State,
    pub(crate) tcp: crate::modules::net_shared_vm::Transport,
    pub(crate) net_sockets: std::collections::HashMap<u64, NetSocket>,
    pub(crate) net_closing_sockets: std::collections::HashMap<u64, quench_runtime::RootId>,
    pub(crate) net_socket_close_completed: Option<quench_runtime::RootId>,
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
    pub(crate) events: std::sync::mpsc::Receiver<ChildProcessEvent>,
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
        self.builtin_cache.clear();
        self.commonjs_cache = None;
        self.module_parent_api = None;
        self.active_esm_requires.clear();
        self.buffer_prototype = None;
        self.primordials_module = None;
        self.event_targets_api = None;
        self.process_module = None;
        self.assert_module = None;
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
        self.net_handle_identities.clear();
        self.net_handle_close_callbacks.clear();
        self.fs_watch = crate::modules::fs_watch_shared_vm::State::default();
        self.tls_contexts = crate::modules::tls_shared_vm::contexts::State::default();
        self.fs_stats = crate::modules::fs_shared_vm::stat::types::State::default();
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
            async_hooks: crate::modules::async_hooks_state::SharedAsyncHooksState::new(
                async_identity,
            ),
            scheduler: crate::modules::shared_event_loop::SharedEventLoop::new(),
            fs,
            fs_watch: crate::modules::fs_watch_shared_vm::State::default(),
            tls_contexts: crate::modules::tls_shared_vm::contexts::State::default(),
            fs_stats: crate::modules::fs_shared_vm::stat::types::State::default(),
            fs_stat_watch: crate::modules::fs_stat_watch_shared_vm::State::default(),
            cwd,
            buffer_prototype: None,
            builtin_cache: std::collections::HashMap::new(),
            commonjs_cache: None,
            module_parent_api: None,
            active_esm_requires: std::collections::HashMap::new(),
            primordials_module: None,
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
            stream_high_water_marks: crate::modules::stream_state::HighWaterMarks::default(),
            unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode::Throw,
            output: None,
            net_auto_select_family: crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY,
            net_auto_select_family_attempt_timeout:
                crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT,
            process_module: None,
            assert_module: None,
            path_module: None,
            url_constructor: None,
            url_state: None,
            url_search_params_constructor: None,
            timer_handle_api: None,
            timer_duration_warnings: Default::default(),
            exec_argv: Vec::new(),
            fetch: crate::modules::fetch_shared_vm::FetchState::new(),
            diagnostics:
                crate::modules::diagnostics_channel_shared_vm::SharedDiagnosticsState::default(),
            http: crate::modules::http_shared_vm::State::new(),
            tcp: crate::modules::net_shared_vm::Transport::new(),
            net_sockets: std::collections::HashMap::new(),
            net_closing_sockets: std::collections::HashMap::new(),
            net_socket_close_completed: None,
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
            net_pending_writes: Vec::new(),
            net_pending_ends: Vec::new(),
            net_pending_destroys: Vec::new(),
        }
    }

    /// Select Node's Script/Module parse goal from its filename and package type.
    pub fn source_kind(path: &std::path::Path) -> Result<quench_runtime::SourceKind, String> {
        super::shared_vm::source_kind(path)
    }

    /// The last VFS CLI switch owns feature availability for this logical process.
    pub(crate) fn vfs_enabled(&self) -> bool {
        let state = self.shared_state.borrow();
        let mut arguments = state.exec_argv.iter();
        let mut enabled = false;
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--experimental-vfs" => enabled = true,
                "--no-experimental-vfs" => enabled = false,
                // The shared CLI retains option values and eval source in execArgv.
                // They are input, even when their text matches a VFS switch.
                "--trace-event-categories" | "--trace-event-file-pattern" | "-r" | "--require" => {
                    let _ = arguments.next();
                }
                "-e" | "--eval" | "-p" | "--print" | "-pe" | "-ep" => break,
                _ => {}
            }
        }
        enabled
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
        let mode =
            crate::modules::process_state::UnhandledRejectionMode::from_exec_argv(&exec_argv)?;
        let net_timeout =
            crate::modules::net_config::auto_select_family_attempt_timeout_from_exec_argv(
                &exec_argv,
            );
        let net_family = crate::modules::net_config::auto_select_family_from_exec_argv(&exec_argv);
        let mut shared = self.shared_state.borrow_mut();
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
