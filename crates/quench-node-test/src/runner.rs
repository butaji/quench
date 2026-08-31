//! The canonical Node test runner entry points.
//!
//! `run_file` is the single command-line entry point used by the
//! `run-stages` / `run-all` / `triage` binaries. The runner never
//! forks, never rewrites the host, and never inspects the host
//! state directly.
//!
//! Each fixture runs on a dedicated thread so engine thread-local
//! state (realm globals, promise queues) starts clean for every
//! test, mirroring node's own runner which spawns a child process
//! per test file.

use std::path::Path;
use std::sync::Arc;

use quench_runtime::vm::OutputSink;

use crate::reader::{NodeFixture, NodeOutcome, NodeRunner};

/// Thread stack for fixture runs; deeply recursive fixtures need
/// more than the default spawned-thread stack.
const FIXTURE_STACK_SIZE: usize = 256 * 1024 * 1024;

pub struct NodeTestRunner {
    sink: OutputSink,
}

impl Default for NodeTestRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeTestRunner {
    pub fn new() -> Self {
        let sink: OutputSink = Arc::new(|line| {
            println!("{line}");
        });
        Self { sink }
    }

    pub fn with_output_sink(mut self, sink: OutputSink) -> Self {
        self.sink = sink;
        self
    }

    pub fn run_file(&mut self, path: &Path) -> NodeOutcome {
        let fixture = match NodeFixture::from_path(path.to_path_buf()) {
            Ok(f) => f,
            Err(e) => return NodeOutcome::Fail { reason: e },
        };
        self.run_fixture(fixture)
    }

    pub fn run_file_with_args(&mut self, path: &Path, argv: Vec<String>) -> NodeOutcome {
        self.run_file_with_options(path, argv, Vec::new())
    }

    /// Run a self-reexec with Node executable flags kept separate from script
    /// arguments. Flags belong in `process.execArgv`; ordinary values belong in
    /// `process.argv`, so preserving that boundary is part of the observable
    /// host contract.
    pub fn run_file_with_options(
        &mut self,
        path: &Path,
        argv: Vec<String>,
        exec_argv: Vec<String>,
    ) -> NodeOutcome {
        let mut fixture = match NodeFixture::from_path(path.to_path_buf()) {
            Ok(fixture) => fixture,
            Err(error) => return NodeOutcome::Fail { reason: error },
        };
        fixture.exec_argv.extend(exec_argv);
        fixture.argv.extend(argv);
        self.run_fixture(fixture)
    }

    pub fn run_source(&mut self, source: &str) -> NodeOutcome {
        let fixture =
            NodeFixture::from_source(std::path::PathBuf::from("<source>"), source.to_string());
        self.run_fixture(fixture)
    }

    fn run_fixture(&mut self, fixture: NodeFixture) -> NodeOutcome {
        let sink = self.sink.clone();
        let handle = std::thread::Builder::new()
            .stack_size(FIXTURE_STACK_SIZE)
            .spawn(move || NodeRunner::new().with_output_sink(sink).run(&fixture));
        match handle {
            Ok(join) => match join.join() {
                Ok(outcome) => outcome,
                Err(_) => NodeOutcome::Fail {
                    reason: "fixture thread panicked".to_string(),
                },
            },
            Err(error) => NodeOutcome::Fail {
                reason: format!("spawn fixture thread: {error}"),
            },
        }
    }
}

pub fn run_file(path: &Path) -> NodeOutcome {
    NodeTestRunner::new().run_file(path)
}
