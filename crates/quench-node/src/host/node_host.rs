//! Shared-runtime Node host state and entry configuration.

use std::cell::RefCell;
use std::rc::Rc;

use crate::shared_run::EntryGoal;

/// Output callback owned by the canonical Node host boundary.
pub type NodeOutputSink = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

pub struct NodeHost {
    pub(crate) shared_state: Rc<RefCell<SharedNodeState>>,
    pub(crate) commonjs_entry: Option<CommonJsEntry>,
}

#[derive(Clone)]
pub(crate) struct CommonJsEntry {
    pub(crate) path: std::path::PathBuf,
    pub(crate) goal: EntryGoal,
}

/// Host-owned state and lifecycle data for the shared VM path.
pub(crate) struct SharedNodeState {
    pub(crate) async_hooks: crate::modules::async_hooks_state::SharedAsyncHooksState,
    pub(crate) scheduler: crate::modules::shared_event_loop::SharedEventLoop,
    pub(crate) fs: crate::modules::fs_state::FsState,
    pub(crate) cwd: crate::modules::process_state::ProcessCwd,
    pub(crate) module_cache: std::collections::HashMap<String, quench_runtime::RootId>,
    /// Immutable startup arguments used to build guest process.argv.
    pub(crate) process_argv: crate::modules::process_state::ProcessArgs,
    pub(crate) process_control: crate::modules::process_state::ProcessControl,
    pub(crate) unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode,
    pub(crate) output: Option<NodeOutputSink>,
    pub(crate) net_auto_select_family_attempt_timeout: u64,
    pub(crate) process_module: Option<quench_runtime::RootId>,
    pub(crate) assert_module: Option<quench_runtime::RootId>,
    pub(crate) path_module: Option<quench_runtime::RootId>,
    pub(crate) url_constructor: Option<quench_runtime::RootId>,
    pub(crate) timer_handle_api: Option<crate::modules::timers_shared_vm::TimerHandleApi>,
    /// Invocation flags supplied by the embedder for this logical process.
    pub(crate) exec_argv: Vec<String>,
    pub(crate) fetch: crate::modules::fetch_shared_vm::FetchState,
    pub(crate) diagnostics: crate::modules::diagnostics_channel_shared_vm::SharedDiagnosticsState,
    pub(crate) http: crate::modules::http_shared_vm::State,
    pub(crate) tcp: crate::modules::net_shared_vm::Transport,
}

impl SharedNodeState {
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
            cwd,
            module_cache: std::collections::HashMap::new(),
            process_argv,
            process_control,
            unhandled_rejection_mode: crate::modules::process_state::UnhandledRejectionMode::Throw,
            output: None,
            net_auto_select_family_attempt_timeout:
                crate::modules::net_config::DEFAULT_AUTO_SELECT_FAMILY_ATTEMPT_TIMEOUT,
            process_module: None,
            assert_module: None,
            path_module: None,
            url_constructor: None,
            timer_handle_api: None,
            exec_argv: Vec::new(),
            fetch: crate::modules::fetch_shared_vm::FetchState::new(),
            diagnostics:
                crate::modules::diagnostics_channel_shared_vm::SharedDiagnosticsState::default(),
            http: crate::modules::http_shared_vm::State::new(),
            tcp: crate::modules::net_shared_vm::Transport::new(),
        }
    }
}

impl NodeHost {
    pub fn new(argv: Vec<String>) -> Self {
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
        let cwd = crate::modules::process_state::ProcessCwd::new(
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/")),
        );
        Self {
            shared_state: Rc::new(RefCell::new(SharedNodeState::new(
                fs,
                cwd,
                argv,
                process_control,
                async_identity,
            ))),
            commonjs_entry: None,
        }
    }

    /// Select Node's Script/Module parse goal from its filename and package type.
    pub fn source_kind(path: &std::path::Path) -> Result<quench_runtime::SourceKind, String> {
        super::shared_vm::source_kind(path)
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
        let mut shared = self.shared_state.borrow_mut();
        shared.exec_argv = exec_argv;
        shared.unhandled_rejection_mode = mode;
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
