//! Node policy and host state for the shared runtime.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use quench_runtime::RootId;

#[path = "host/shared_vm.rs"]
pub(crate) mod shared_vm;

pub struct NodeHost {
    state: Rc<RefCell<HostState>>,
    pub(crate) commonjs_entry: Option<CommonJsEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryGoal {
    Node,
    CommonJs,
}

#[derive(Clone)]
pub(crate) struct CommonJsEntry {
    pub(crate) path: PathBuf,
    pub(crate) goal: EntryGoal,
}

pub type ModuleCache = HashMap<String, RootId>;
pub type ProcessModule = RootId;

pub(crate) struct HostState {
    pub(crate) event_loop: crate::modules::event_loop::EventLoop,
    pub(crate) process: crate::modules::process::ProcessState,
    pub(crate) exec_argv: Vec<String>,
    pub(crate) net: crate::modules::net::NetState,
    pub(crate) fetch: crate::modules::fetch_shared_vm::FetchState,
    pub(crate) http: crate::modules::http::HttpState,
    pub(crate) module_cache: ModuleCache,
    pub(crate) process_module: Option<ProcessModule>,
    pub(crate) assert_module: Option<RootId>,
    pub(crate) path_module: Option<RootId>,
}

impl NodeHost {
    pub fn new(argv: Vec<String>) -> Self {
        let state = HostState {
            event_loop: crate::modules::event_loop::EventLoop::new(),
            process: crate::modules::process::ProcessState::new(argv),
            exec_argv: Vec::new(),
            net: crate::modules::net::NetState::new(),
            fetch: crate::modules::fetch_shared_vm::FetchState::new(),
            http: crate::modules::http::HttpState::new(),
            module_cache: HashMap::new(),
            process_module: None,
            assert_module: None,
            path_module: None,
        };
        Self {
            state: Rc::new(RefCell::new(state)),
            commonjs_entry: None,
        }
    }

    pub fn with_commonjs_entry(self, path: PathBuf) -> Self {
        self.with_commonjs_entry_goal(path, EntryGoal::CommonJs)
    }

    pub fn with_commonjs_entry_goal(mut self, path: PathBuf, goal: EntryGoal) -> Self {
        self.commonjs_entry = Some(CommonJsEntry { path, goal });
        self
    }

    pub fn with_exec_argv(self, exec_argv: Vec<String>) -> Self {
        self.state.borrow_mut().exec_argv = exec_argv;
        self
    }

    pub(crate) fn state(&self) -> Rc<RefCell<HostState>> {
        Rc::clone(&self.state)
    }

    pub fn source_kind(path: &Path) -> Result<quench_runtime::SourceKind, String> {
        shared_vm::source_kind(path)
    }
}
